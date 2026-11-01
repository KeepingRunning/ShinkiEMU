/* Query the real packaged core from a clean native process, without game startup. */
#include "shinki_emu.h"
#include <stdio.h>
#ifdef _WIN32
#include <windows.h>
#else
#include <dlfcn.h>
#endif

#ifdef _WIN32
int wmain(int argc, wchar_t **argv) {
#else
int main(int argc, char **argv) {
#endif
    if(argc != 2) return 2;
#ifdef _WIN32
    HMODULE library = LoadLibraryExW(argv[1], NULL,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if(!library) {
        fprintf(stderr, "LoadLibraryExW failed: %lu\n", GetLastError());
        return 3;
    }
    ShinkiEmuQuery query = (ShinkiEmuQuery)GetProcAddress(library, "shinki_emu_query");
#else
    void *library = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if(!library) {
        fprintf(stderr, "dlopen failed: %s\n", dlerror());
        return 3;
    }
    ShinkiEmuQuery query = (ShinkiEmuQuery)dlsym(library, "shinki_emu_query");
#endif
    if(!query) return 4;
    const ShinkiEmuCoreApi *api = NULL;
    ShinkiEmuStatus supported = query(ShinkiEmuABI_VERSION, &api);
    if(supported != ShinkiEmuOK || !api ||
       api->header.size != sizeof(ShinkiEmuCoreApi) ||
       api->header.version != ShinkiEmuABI_VERSION ||
       !api->initialize || !api->describe || !api->probe || !api->open ||
       !api->input || !api->control || !api->state || !api->frame ||
       !api->cancel || !api->close || !api->extension) return 5;
    const ShinkiEmuCoreApi *rejected = api;
    ShinkiEmuStatus unsupported = query(ShinkiEmuABI_VERSION + 1, &rejected);
    ShinkiEmuStatus null_output = query(ShinkiEmuABI_VERSION, NULL);
    if(unsupported != ShinkiEmuUNSUPPORTED || rejected != NULL ||
       null_output != ShinkiEmuINVALID_ARGUMENT) return 6;
    printf("{\"abi\":%u,\"table_size\":%u,\"callbacks\":11,"
           "\"unsupported_version_status\":%d,\"null_output_status\":%d,"
           "\"game_or_session_opened\":false}\n",
           (unsigned int)ShinkiEmuABI_VERSION, (unsigned int)sizeof(ShinkiEmuCoreApi),
           unsupported, null_output);
    return 0;
}
