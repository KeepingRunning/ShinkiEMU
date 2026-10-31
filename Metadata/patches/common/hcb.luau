-- Standard HCB writer: keep all original instruction PCs, move only growing
-- literals into executable islands. No integer/address guessing or runtime table.
local M = {}
local widths = {[1]=3,[2]=5,[3]=3,[6]=5,[7]=5,[10]=5,[11]=3,[12]=2,[13]=5,
    [15]=3,[16]=2,[17]=3,[18]=2,[21]=3,[22]=2,[23]=3,[24]=2}
local function need(ctx, condition, message)
    if not condition then ctx.fail("INTEGRITY", "HCB: " .. message) end
end
local function encoded(ctx, bytes, source_encoding, target_encoding)
    return ctx.encoding.encode(ctx.encoding.decode(bytes, source_encoding), target_encoding)
end
local function slice(bytes, at, size)
    local result = buffer.create(size)
    buffer.copy(result, 0, bytes, at, size)
    return result
end
function M.native(ctx, original, records, source_encoding, target_encoding)
    local size = original:size()
    need(ctx, size >= 20 and size < 0x800000, "input bounds")
    local bytes = original:read(0, size)
    local finish = buffer.readu32(bytes, 0)
    need(ctx, finish >= 5 and finish + 16 <= size, "descriptor bounds")
    local boundaries = buffer.create(finish)
    local changes, replacements, count = {}, {}, 0
    for _, row in records do
        need(ctx, not replacements[row.offset], "duplicate replacement")
        replacements[row.offset] = row.text
    end
    local at, island_end = 4, finish
    while at < finish do
        ctx.check_cancelled()
        buffer.writeu8(boundaries, at, 1)
        local op = buffer.readu8(bytes, at)
        need(ctx, op <= 39, "unknown opcode")
        local length = widths[op] or 1
        if op == 14 then
            need(ctx, at + 2 <= finish, "literal header bounds")
            local n = buffer.readu8(bytes, at + 1)
            length = n + 2
            need(ctx, n > 0 and at + length <= finish, "literal bounds")
            need(ctx, buffer.readu8(bytes, at + length - 1) == 0, "literal terminator")
            local replacement = replacements[at + 2]
            local text
            if replacement then
                text = encoded(ctx, replacement, "utf8", target_encoding)
                replacements[at + 2] = nil
                count += 1
            else
                text = encoded(ctx, slice(bytes, at + 2, n - 1), source_encoding, target_encoding)
            end
            local len = buffer.len(text)
            need(ctx, len <= 254 and not string.find(buffer.tostring(text), "\0", 1, true), "literal encoding bounds")
            local destination = at
            if len + 3 > length then
                need(ctx, length >= 5 and at + length < finish, "growing literal needs jump space and continuation")
                destination = island_end
                island_end += len + 8 -- PushString/u8/NUL and Jmp/u32
            end
            table.insert(changes, {at=at, size=length, text=text, destination=destination})
        end
        need(ctx, at + length <= finish, "instruction bounds")
        at += length
    end
    need(ctx, at == finish and next(replacements) == nil, "replacement is not a literal boundary")
    need(ctx, island_end < 0x800000, "native read-flag address limit")
    -- Every direct code edge and entry must remain at a real original instruction.
    at = 4
    while at < finish do
        local op = buffer.readu8(bytes, at)
        if op == 2 or op == 6 or op == 7 then
            local target = buffer.readu32(bytes, at + 1)
            need(ctx, target >= 4 and target < finish and buffer.readu8(boundaries, target) == 1, "code edge is not an instruction boundary")
        end
        at += if op == 14 then 2 + buffer.readu8(bytes, at + 1) else widths[op] or 1
    end
    local entry = buffer.readu32(bytes, finish)
    need(ctx, entry >= 4 and entry < finish and buffer.readu8(boundaries, entry) == 1, "entry boundary")
    -- The descriptor has length-prefixed title/syscall strings, not executable code.
    local descriptor = {slice(bytes, finish, 10)}
    at = finish + 10
    local function descriptor_string()
        need(ctx, at < size, "descriptor string header")
        local n = buffer.readu8(bytes, at)
        need(ctx, n > 0 and at + 1 + n <= size and buffer.readu8(bytes, at + n) == 0, "descriptor string bounds")
        local value = encoded(ctx, slice(bytes, at + 1, n - 1), source_encoding, target_encoding)
        need(ctx, buffer.len(value) <= 254, "descriptor string length")
        local out = buffer.create(buffer.len(value) + 2)
        buffer.writeu8(out, 0, buffer.len(value) + 1)
        buffer.copy(out, 1, value)
        table.insert(descriptor, out)
        at += n + 1
    end
    descriptor_string()
    need(ctx, at + 2 <= size, "syscall count bounds")
    local calls = buffer.readu16(bytes, at)
    table.insert(descriptor, slice(bytes, at, 2))
    at += 2
    for _ = 1, calls do
        need(ctx, at < size, "syscall args bounds")
        table.insert(descriptor, slice(bytes, at, 1))
        at += 1
        descriptor_string()
    end
    need(ctx, at + 2 == size and buffer.readu16(bytes, at) == 0, "custom syscall/trailing descriptor unsupported")
    table.insert(descriptor, slice(bytes, at, 2))
    local total = island_end
    for _, part in descriptor do total += buffer.len(part) end
    local output = buffer.create(total)
    buffer.copy(output, 0, bytes, 0, finish)
    buffer.writeu32(output, 0, island_end)
    for _, change in changes do
        ctx.check_cancelled()
        local p, dest, len = change.at, change.destination, buffer.len(change.text)
        buffer.fill(output, p, 0, change.size) -- opcode 0 is Nop, including old payload bytes
        if dest ~= p then
            buffer.writeu8(output, p, 6)
            buffer.writeu32(output, p + 1, dest)
        end
        buffer.writeu8(output, dest, 14)
        buffer.writeu8(output, dest + 1, len + 1)
        buffer.copy(output, dest + 2, change.text)
        if dest ~= p then
            buffer.writeu8(output, dest + len + 3, 6)
            buffer.writeu32(output, dest + len + 4, p + change.size)
        end
    end
    at = island_end
    for _, part in descriptor do buffer.copy(output, at, part) at += buffer.len(part) end
    return output, {translated=count, islands=island_end-finish}
end
return M
