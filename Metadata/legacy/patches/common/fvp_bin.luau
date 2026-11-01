-- Standard FVP BIN index conversion. Exact-width names retain all physical
-- offsets, entry sizes, alignment and payload bytes; other layouts are rejected.
local M = {}
local function need(ctx, condition, message)
    if not condition then ctx.fail("INTEGRITY", "FVP BIN: " .. message) end
end
function M.native(ctx, root, path, source_encoding, target_encoding)
    local source = ctx.open(root, path)
    local size = source:size()
    need(ctx, size >= 8, "header bounds")
    local header = source:read(0, 8)
    local count, length = buffer.readu32(header, 0), buffer.readu32(header, 4)
    need(ctx, count <= 1024 * 1024 and length <= 16 * 1024 * 1024, "index bounds")
    local names_at = 8 + count * 12
    local data_at = names_at + length
    need(ctx, data_at <= size, "metadata bounds")
    local names = source:read(names_at, length)
    local encoded = buffer.create(length)
    local boundaries, used, at, changed = {}, {}, 0, 0
    while at < length do
        local finish = at
        while finish < length and buffer.readu8(names, finish) ~= 0 do finish += 1 end
        need(ctx, finish < length, "unterminated name")
        local text = ctx.encoding.decode(buffer.fromstring(buffer.readstring(names, at, finish-at)), source_encoding)
        local value = ctx.encoding.encode(text, target_encoding)
        need(ctx, buffer.len(value) == finish-at, "encoded name width changes; index rebuild required")
        boundaries[at] = string.lower(text) -- native lookup folds ASCII only
        buffer.copy(encoded, at, value)
        if buffer.readstring(names, at, finish-at) ~= buffer.tostring(value) then changed += 1 end
        at = finish + 1
    end
    local entries = source:read(8, count * 12)
    for index = 0, count - 1 do
        ctx.check_cancelled()
        local offset = buffer.readu32(entries, index * 12)
        local position = buffer.readu32(entries, index * 12 + 4)
        local bytes = buffer.readu32(entries, index * 12 + 8)
        local key = boundaries[offset]
        need(ctx, key ~= nil and key ~= "", "entry name boundary")
        need(ctx, not used[key], "native resource name collision")
        used[key] = true
        need(ctx, position >= data_at and position + bytes <= size, "entry payload bounds")
    end
    if changed == 0 then
        ctx.copy(root, path, path)
    else
        local output = ctx.create(path)
        output:write(0, header)
        output:write(8, entries)
        output:write(names_at, encoded)
        at = data_at
        while at < size do
            ctx.check_cancelled()
            local bytes = math.min(4 * 1024 * 1024, size-at)
            output:write(at, source:read(at, bytes))
            at += bytes
        end
        output:close()
    end
    return {entries=count, changed=changed}
end
return M
