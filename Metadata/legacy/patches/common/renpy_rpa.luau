-- Offline conversion of explicitly fingerprinted length-first RPA-3.0 indexes.
-- Payload positions, header bytes, archive priority, and Python value types stay intact.
local rpa = {}
local HEADER_SIZE = 34
local READ_MAX = 16 * 1024 * 1024
local INDEX_MAX = 64 * 1024 * 1024
local EXACT_MAX = 9007199254740991

local function need(ctx, condition, kind, message)
    if not condition then ctx.fail(kind, message) end
end

local function hex(ctx, text, maximum)
    need(ctx, string.match(text, "^[0-9a-fA-F]+$") ~= nil,
        "UNSUPPORTED_INPUT", "RPA header contains an invalid hexadecimal field")
    local result = 0
    for position = 1, #text do
        local digit = tonumber(string.sub(text, position, position), 16)
        need(ctx, result <= math.floor((maximum - digit) / 16),
            "BOUNDS", "RPA header integer exceeds its supported range")
        result = result * 16 + digit
    end
    return result
end

local function uint32(ctx, node)
    need(ctx, type(node) == "table" and node.kind == "int"
        and type(node.value) == "string" and string.match(node.value, "^%d+$") ~= nil,
        "UNSUPPORTED_INPUT", "RPA chunk coordinates must be unsigned integer nodes")
    local value = tonumber(node.value)
    need(ctx, value ~= nil and value <= 4294967295 and value == math.floor(value),
        "BOUNDS", "RPA chunk coordinate exceeds uint32")
    return value
end

local function resource_path(ctx, node)
    need(ctx, type(node) == "table" and (node.kind == "string" or node.kind == "bytes"),
        "UNSUPPORTED_INPUT", "RPA resource keys must be strings or bytes")
    local path = if node.kind == "bytes" then buffer.tostring(node.value) else node.value
    need(ctx, type(path) == "string" and #path > 0 and utf8.len(path) ~= nil,
        "UNSUPPORTED_INPUT", "RPA resource path must be nonempty UTF-8")
    need(ctx, not string.find(path, "[\\:%z\1-\31\127]")
        and string.sub(path, 1, 1) ~= "/" and string.sub(path, -1) ~= "/"
        and not string.find(path, "//", 1, true),
        "UNSUPPORTED_INPUT", "RPA resource path is not a safe relative path")
    for part in string.gmatch(path, "[^/]+") do
        need(ctx, part ~= "." and part ~= "..", "UNSUPPORTED_INPUT",
            "RPA resource path contains a traversal component")
    end
    return path
end

-- Selected members are reconstructed only after their original extents validate.
local function member_bytes(ctx, source, chunks, key)
    local size = 0
    for _, chunk in ipairs(chunks.items) do
        local items = chunk.items
        local length = bit32.bxor(uint32(ctx, items[2]), key)
        local prefix = items[3]
        local prefix_size = if prefix and prefix.kind == "bytes" then buffer.len(prefix.value) else 0
        need(ctx, prefix_size <= READ_MAX - size and length <= READ_MAX - size - prefix_size,
            "BOUNDS", "Selected RPA member exceeds 16 MiB")
        size += prefix_size + length
    end
    local bytes = buffer.create(size)
    local at = 0
    for _, chunk in ipairs(chunks.items) do
        ctx.check_cancelled()
        local items = chunk.items
        local prefix = items[3]
        if prefix and prefix.kind == "bytes" then
            buffer.copy(bytes, at, prefix.value)
            at += buffer.len(prefix.value)
        end
        local offset = bit32.bxor(uint32(ctx, items[1]), key)
        local length = bit32.bxor(uint32(ctx, items[2]), key)
        buffer.copy(bytes, at, source:read(offset, length))
        at += length
    end
    return bytes
end

local function normalize_index(ctx, index, key, index_offset, source, replace)
    need(ctx, type(index) == "table" and index.kind == "dict", "UNSUPPORTED_INPUT",
        "RPA index root must be a dictionary")
    local paths = {}
    local retained = if replace then {} else nil
    if replace then
        need(ctx, type(replace) == "table", "INVALID_REQUEST", "RPA replacements must be a callback map")
        for path, callback in pairs(replace) do
            resource_path(ctx, { kind = "string", value = path })
            need(ctx, type(callback) == "function", "INVALID_REQUEST", "RPA replacement must be a callback")
        end
    end
    for _, entry in ipairs(index.entries) do
        ctx.check_cancelled()
        local path = resource_path(ctx, entry.key)
        need(ctx, not paths[path], "INTEGRITY", "RPA index contains duplicate resource paths")
        paths[path] = true
        local chunks = entry.value
        need(ctx, type(chunks) == "table" and chunks.kind == "list",
            "UNSUPPORTED_INPUT", "RPA resource chunks must be a list")
        for _, chunk in ipairs(chunks.items) do
            ctx.check_cancelled()
            need(ctx, type(chunk) == "table" and (chunk.kind == "tuple" or chunk.kind == "list")
                and (#chunk.items == 2 or #chunk.items == 3),
                "UNSUPPORTED_INPUT", "RPA chunks must contain two or three fields")
            local items = chunk.items
            local length = bit32.bxor(uint32(ctx, items[1]), key)
            local offset = bit32.bxor(uint32(ctx, items[2]), key)
            if #items == 3 then
                need(ctx, type(items[3]) == "table"
                    and (items[3].kind == "bytes" or items[3].kind == "none"),
                    "UNSUPPORTED_INPUT", "RPA chunk prefix must be bytes or None")
            end
            need(ctx, offset >= HEADER_SIZE and offset <= index_offset
                and length <= index_offset - offset,
                "BOUNDS", "RPA resource extent intersects the header or index")
            -- XOR commutes with this swap; retain the original integer nodes.
            items[1], items[2] = items[2], items[1]
        end
        if replace then
            local callback = replace[path]
            if callback then
                callback(ctx, member_bytes(ctx, source, chunks, key))
            else
                table.insert(retained, entry)
            end
        end
    end
    if replace then
        for path in pairs(replace) do
            need(ctx, paths[path] == true, "INTEGRITY", "Required RPA replacement member is missing: " .. path)
        end
        index.entries = retained
    end
end

-- Read official RPC2 containers without interpreting or instantiating their AST.
-- Fingerprints identify both the complete compiled member and its Python source.
function rpa.rpyc_python_source(ctx, bytes, spec)
    need(ctx, buffer.len(bytes) == spec.size and ctx.hash.sha256(bytes) == spec.sha256,
        "INTEGRITY", "RPYC member size or SHA-256 differs from the selected source")
    local size = buffer.len(bytes)
    need(ctx, size >= 22 and buffer.readstring(bytes, 0, 10) == "RENPY RPC2",
        "UNSUPPORTED_INPUT", "RPYC member is not an official RPC2 container")
    local slots = {}
    local at = 10
    while true do
        ctx.check_cancelled()
        need(ctx, at <= size - 12, "BOUNDS", "RPC2 slot table is truncated")
        local slot = buffer.readu32(bytes, at)
        local offset = buffer.readu32(bytes, at + 4)
        local length = buffer.readu32(bytes, at + 8)
        at += 12
        if slot == 0 then
            need(ctx, offset == 0 and length == 0, "INTEGRITY", "RPC2 slot terminator is invalid")
            break
        end
        need(ctx, slots[slot] == nil, "INTEGRITY", "RPC2 slot identifier is duplicated")
        need(ctx, offset <= size and length > 0 and length <= size - offset,
            "BOUNDS", "RPC2 slot extends outside its member")
        slots[slot] = { offset = offset, length = length }
    end
    local spans = {}
    for _, slot in pairs(slots) do
        need(ctx, slot.offset >= at, "BOUNDS", "RPC2 slot overlaps its table")
        table.insert(spans, slot)
    end
    table.sort(spans, function(a, b) return a.offset < b.offset end)
    for position = 2, #spans do
        need(ctx, spans[position].offset >= spans[position - 1].offset + spans[position - 1].length,
            "BOUNDS", "RPC2 slots overlap")
    end
    local slot = slots[2]
    need(ctx, slot ~= nil, "INTEGRITY", "Required RPC2 slot 2 is missing")
    need(ctx, slot.length <= READ_MAX, "BOUNDS", "RPC2 compressed slot exceeds 16 MiB")
    local compressed = buffer.create(slot.length)
    buffer.copy(compressed, 0, bytes, slot.offset, slot.length)
    local plain = ctx.codec.inflate_bounded(compressed, INDEX_MAX)
    local found = nil
    local count = 0
    for _, text in ipairs(ctx.pickle.strings(plain)) do
        ctx.check_cancelled()
        if #text == spec.source_size and ctx.hash.sha256(buffer.fromstring(text)) == spec.source_sha256 then
            found = text
            count += 1
        end
    end
    need(ctx, count == 1, "INTEGRITY", "RPYC Python source size/SHA-256 must match exactly once")
    return found
end

-- The caller selects source/destination and pins the exact input edition.
function rpa.normalize(ctx, item)
    local root = item.root or "source"
    need(ctx, type(item.fingerprint) == "table"
        and type(item.fingerprint.size) == "number"
        and type(item.fingerprint.sha256) == "string"
        and #item.fingerprint.sha256 == 64
        and string.match(item.fingerprint.sha256, "^[0-9a-f]+$") ~= nil,
        "INVALID_REQUEST", "RPA normalization requires a size and SHA-256 fingerprint")
    local source = ctx.open(root, item.source)
    need(ctx, source:size() == item.fingerprint.size and source:sha256() == item.fingerprint.sha256,
        "UNSUPPORTED_INPUT", "RPA input differs from the selected edition fingerprint")
    need(ctx, source:size() >= HEADER_SIZE, "BOUNDS", "RPA header is truncated")
    local header = buffer.tostring(source:read(0, HEADER_SIZE))
    need(ctx, string.sub(header, 1, 8) == "RPA-3.0 "
        and string.sub(header, 25, 25) == " " and string.sub(header, 34, 34) == "\n",
        "UNSUPPORTED_INPUT", "RPA input does not have a standard RPA-3.0 header")
    local index_offset = hex(ctx, string.sub(header, 9, 24), EXACT_MAX)
    local key = hex(ctx, string.sub(header, 26, 33), 4294967295)
    need(ctx, index_offset >= HEADER_SIZE and index_offset < source:size(),
        "BOUNDS", "RPA index offset exceeds the archive body")
    local compressed_size = source:size() - index_offset
    need(ctx, compressed_size <= READ_MAX, "BOUNDS", "RPA compressed index exceeds 16 MiB")
    local compressed = source:read(index_offset, compressed_size)
    local plain = ctx.codec.inflate_bounded(compressed, INDEX_MAX)
    compressed = nil
    local index = ctx.pickle.decode(plain)
    plain = nil
    normalize_index(ctx, index, key, index_offset, source, item.replace)
    local encoded = ctx.pickle.encode(index)
    index = nil
    local replacement = ctx.codec.deflate(encoded)
    encoded = nil
    need(ctx, buffer.len(replacement) <= READ_MAX, "BOUNDS", "Normalized RPA index exceeds 16 MiB")
    need(ctx, index_offset <= EXACT_MAX - buffer.len(replacement), "BOUNDS",
        "Normalized RPA extent exceeds the exact integer range")
    local directory = string.match(item.destination, "^(.*)/[^/]+$")
    if directory and directory ~= "" then ctx.mkdir(directory) end
    ctx.copy(root, item.source, item.destination)
    local output = ctx.edit(item.destination)
    output:write(index_offset, replacement)
    output:truncate(index_offset + buffer.len(replacement))
    output:close()
end

return rpa
