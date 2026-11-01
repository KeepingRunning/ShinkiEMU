-- The private legacy dictionary wrapper and pinned warning flow.
-- Literal encoding and bounded executable layout belong to common/hcb.luau.
local M = {}

local function need(ctx, condition, message)
    if not condition then ctx.fail("INTEGRITY", message) end
end


function M.dictionary(ctx, source)
    local size = source:size()
    need(ctx, size >= 16 and size <= 16 * 1024 * 1024, "FVP dictionary wrapper bounds")
    local wrapper = source:read(0, size)
    need(ctx, buffer.readu32(wrapper, 0) == 0, "FVP dictionary wrapper version")
    local expanded = buffer.readu32(wrapper, 8)
    local compressed = buffer.readu32(wrapper, 12)
    need(ctx, compressed == size - 16, "FVP dictionary compressed bounds")
    local compressed_data = buffer.create(compressed)
    buffer.copy(compressed_data, 0, wrapper, 16, compressed)
    local data = ctx.codec.inflate(compressed_data, expanded)
    need(ctx, expanded >= 4, "FVP dictionary count missing")
    local count = buffer.readu32(data, 0)
    need(ctx, count <= math.floor((expanded - 4) / 7), "FVP dictionary count bounds")
    local records, seen, at = {}, {}, 4
    for _ = 1, count do
        ctx.check_cancelled()
        need(ctx, at + 6 <= expanded, "FVP dictionary record header bounds")
        local offset, len = buffer.readu32(data, at), buffer.readu16(data, at + 4)
        at += 6
        need(ctx, len > 0 and at + len <= expanded, "FVP active dictionary record bounds")
        need(ctx, not seen[offset], "FVP duplicate dictionary offset")
        need(ctx, buffer.readu8(data, at + len - 1) == 0, "FVP dictionary terminator missing")
        local encoded = buffer.fromstring(buffer.readstring(data, at, len - 1))
        local text = ctx.encoding.decode(encoded, "gbk")
        need(ctx, not string.find(text, "\0", 1, true), "FVP dictionary embedded NUL")
        seen[offset] = true
        table.insert(records, { offset = offset, text = ctx.encoding.encode(text, "utf8") })
        at += len
    end
    need(ctx, at == expanded, "FVP dictionary trailing data")
    table.sort(records, function(a, b) return a.offset < b.offset end)
    return records
end

function M.warning(ctx, bytes, spec)
    local finish = buffer.readu32(bytes, 0)
    need(ctx, buffer.readu8(bytes, spec.site) == 2 and buffer.readu32(bytes, spec.site + 1) == spec.original_call,
        "FVP warning original call mismatch")
    local tail = buffer.create(40)
    buffer.writeu8(tail, 0, 2)
    buffer.writeu32(tail, 1, spec.original_call)
    buffer.writeu8(tail, 5, 11)
    buffer.writeu16(tail, 6, 250)
    buffer.writeu8(tail, 8, 14)
    buffer.writeu8(tail, 9, 8)
    -- ASCII is valid in every supported native script encoding.
    buffer.writestring(tail, 10, "warning\0")
    buffer.writeu8(tail, 18, 12)
    buffer.writeu8(tail, 19, 5)
    buffer.writeu8(tail, 20, 8)
    buffer.writeu8(tail, 21, 2)
    buffer.writeu32(tail, 22, spec.helper)
    buffer.writeu8(tail, 26, 11)
    buffer.writeu16(tail, 27, 3000)
    buffer.writeu8(tail, 29, 3)
    buffer.writeu16(tail, 30, spec.wait_syscall)
    buffer.writeu8(tail, 32, 3)
    buffer.writeu16(tail, 33, spec.yield_syscall)
    buffer.writeu8(tail, 35, 6)
    buffer.writeu32(tail, 36, spec.site + 5)
    buffer.writeu8(bytes, spec.site, 6)
    buffer.writeu32(bytes, spec.site + 1, finish)
    local final = buffer.create(buffer.len(bytes) + 40)
    buffer.copy(final, 0, bytes, 0, finish)
    buffer.copy(final, finish, tail)
    buffer.copy(final, finish + 40, bytes, finish, buffer.len(bytes) - finish)
    buffer.writeu32(final, 0, finish + 40)
    return final
end


return M
