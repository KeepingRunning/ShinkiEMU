-- This edition's public MIT loader stores RPA3 chunk coordinates length-first.
-- Normalize its archives and resolution module for the official Ren'Py runtime.
local archives = {
    { name = "audio", size = 854083545, sha256 = "acaa87090ffbe319d5e19f27debc2f6ed4b47a55ab274f4b9d303c3e6e5af20e" },
    { name = "code", size = 22569800, sha256 = "6d3639953a96283f3c454bab9fa85fdc6c161ddb67f758407ff563d85921d98c" },
    { name = "font", size = 88011039, sha256 = "f79b6e288628001f5bbf652f0751192c059467d38d84f94716f68b63ad5653fc" },
    { name = "gui", size = 90728715, sha256 = "3e349595eab5ecc74e400d70b8fc8584759aaf2e8afd1a7164e4cea559db921a" },
    { name = "image", size = 848689883, sha256 = "bdcc697b04e12bd97762fb62429be13b54ba4124dc8dcfe7579f92fcd16ffb75" },
    { name = "movie", size = 1225177451, sha256 = "f968641e09acc09b59b210e49ee51f0b73078775565e6009c58efb85101557ac" },
    { name = "patch", size = 614938128, sha256 = "0477ffa7c8ad07e80d6558ff4dc3e7ed7d0af323ac21a68d9263e8b6a7adf5ba" },
}

local resolution_member = "kagami_games/code/system/force_720p.rpyc"

local function standard_resolution(ctx, bytes)
    local rpa = ctx.load("common/renpy_rpa.luau")
    local source = rpa.rpyc_python_source(ctx, bytes, {
        size = 1386,
        sha256 = "95e32e5243470f3dc25013b557696b7f84c12b7caf9847c087bb2fc4dcc76ceb",
        source_size = 115,
        source_sha256 = "2c75b53935137167151018c0f2245d03b262f5faa66e9d46be24d0de0f3ddf7f",
    })
    local text = "define config.physical_width = 1280\n"
        .. "define config.physical_height = 720\n"
        .. "define kagami_hd_text_enabled = True\n\n"
        .. "init 100 python:\n    " .. string.gsub(source, "\n", "\n    ") .. "\n"
    ctx.mkdir("game/kagami_games/code/system")
    local output = ctx.create("game/kagami_games/code/system/force_720p.rpy")
    output:write(0, buffer.fromstring(text))
    output:close()
end

return {
    id = "renpy-himanatsu",
    version = 2,
    title = "HimaNatsu 标准 Ren'Py 资源",
    apply = function(ctx)
        local version = ctx.open("source", "game/script_version.txt")
        if version:size() > 64 then
            ctx.fail("UNSUPPORTED_INPUT", "Ren'Py script version does not match this edition")
        end
        local text = buffer.tostring(version:read(0, version:size()))
        if not string.match(text, "^%(%s*8%s*,%s*5%s*,%s*2%s*%)%s*$") then
            ctx.fail("UNSUPPORTED_INPUT", "Ren'Py script version does not match this edition")
        end
        local rpa = ctx.load("common/renpy_rpa.luau")
        for index, archive in ipairs(archives) do
            ctx.check_cancelled()
            local path = "game/" .. archive.name .. ".rpa"
            rpa.normalize(ctx, {
                source = path,
                destination = path,
                fingerprint = { size = archive.size, sha256 = archive.sha256 },
                replace = if archive.name == "code" then {
                    [resolution_member] = standard_resolution,
                } else nil,
            })
            collectgarbage("collect")
            ctx.progress("Normalizing standard Ren'Py archives", index, #archives)
        end
        -- Explicit loose-file allowlist: no save/cache, launcher, DLL, or runtime.
        ctx.copy("source", "game/script_version.txt", "game/script_version.txt")
        return { plugin_id = "renpy", configuration = {} }
    end,
}
