#include "shinki_emu.h"
#include <algorithm>
#include <condition_variable>
#include <cstring>
#include <deque>
#include <mutex>
#include <stdexcept>
#include <string>
#include <string_view>
#include <thread>

extern "C" {
int shinki_emu_query(uint32_t, const ShinkiEmuCoreApi **) noexcept;
void shinki_ohos_request_frame();
int shinki_ohos_present(void (*draw)(void *, unsigned char *, int, int, int), void *data);
int shinki_ohos_notify(const char *kind, const char *payload);
}
namespace {
ShinkiEmuBytes bytes(std::string_view v) { return {reinterpret_cast<const uint8_t *>(v.data()), v.size()}; }
struct Event { int kind; double x, y; std::string text; };
class Player {
    std::mutex mutex;
    std::condition_variable wake;
    std::deque<Event> queue;
    bool exiting{};
    const ShinkiEmuCoreApi *api{};
    ShinkiEmuOpenResult game{};
    ShinkiEmuFrameIdentity last{};
    ShinkiEmuError error{};
    int width{}, height{};
    bool foreground{true};
    std::thread worker;

    void check(int status) {
        if (status == ShinkiEmuOK) return;
        std::string message(reinterpret_cast<const char *>(error.message),error.message_len);
        throw std::runtime_error(message.empty() ? "Leaf status " + std::to_string(status) : message);
    }
    void close() {
        if (game.session) {api->cancel(game.session); api->close(game.session,nullptr);}
        game = {}; last = {}; error = {};
    }
    void start(const std::string &selection) {
        close();
        const auto newline = selection.find('\n');
        if (newline == std::string::npos) throw std::runtime_error("Missing game directory");
        const auto path = selection.substr(newline + 1);
        check(shinki_emu_query(3,&api));
        ShinkiEmuDiagnostics diagnostics{};
        check(api->initialize(&diagnostics,&error));
        ShinkiEmuProbeResult probe{};
        check(api->probe(bytes(path),&probe,&error));
        ShinkiEmuConfigEntry settings[] = {{bytes("entry_scene"),{ShinkiEmuVALUE_STRING,0,0,0,bytes("4000")}},
                                          {bytes("script_bank"),{ShinkiEmuVALUE_STRING,0,0,0,bytes("script_cn")}}};
        ShinkiEmuOpenRequest request{};
        request.header={sizeof(request),3}; request.host.header={sizeof(request.host),3};
        request.game_path=bytes(path); request.session_epoch=1;
        request.host.cancellation.is_cancelled=[](void *,uint64_t)->uint32_t{return 0;};
        if (selection.substr(0,newline) == "special") {request.configuration=settings; request.configuration_count=2;}
        check(api->open(&request,&game,&error));
        shinki_ohos_request_frame();
    }
    static void draw(void *data,unsigned char *dest,int w,int h,int stride) {
        auto &f=*static_cast<ShinkiEmuFrameSurface *>(data);
        const auto sw=int(f.info.raster.width),sh=int(f.info.raster.height);
        if (w<=0 || h<=0 || sw<=0 || sh<=0 || stride < w*4) return;
        const double scale=std::min(double(w)/sw,double(h)/sh);
        const int dw=std::max(1,int(sw*scale)), dh=std::max(1,int(sh*scale));
        const int left=(w-dw)/2, top=(h-dh)/2;
        for(int y=0;y<h;++y) {
            auto *row=dest+size_t(y)*stride;
            std::memset(row,0,size_t(w)*4);
            if(y<top || y>=top+dh) continue;
            auto *source=f.cpu.pixels.data+size_t((int64_t(y-top)*sh)/dh)*f.info.stride;
            if(dw==sw) std::memcpy(row+left*4,source,sw*4);
            else for(int x=0;x<dw;++x) std::memcpy(row+(left+x)*4,source+((int64_t(x)*sw)/dw)*4,4);
        }
    }
    void dispatch(const Event &e) {
        if(e.kind==8) {start(e.text);return;}
        if(e.kind==1) {width=int(e.x);height=int(e.y);last={};}
        if(e.kind==6) foreground=e.x!=0;
        if(!game.session) return;
        if(e.kind==0 && foreground) {
            ShinkiEmuSessionState state{};
            check(api->state(game.session,&state,&error));
            if(state.state==ShinkiEmuSESSION_FAILED){error=state.error;check(ShinkiEmuCORE_ERROR);}
            if(state.state==ShinkiEmuSESSION_FINISHED){close();shinki_ohos_notify("closed","");return;}
            ShinkiEmuFrameSurface f{};
            check(api->frame(game.session,last,&f,&error));
            if(f.state==ShinkiEmuFRAME_READY) {
                int result=shinki_ohos_present(draw,&f);
                f.cpu.release(f.cpu.owner);
                if(result) throw std::runtime_error("OHNativeWindow frame submission failed");
                last=f.identity;
            }
        } else if(e.kind==6) {
            check(api->control(game.session,foreground?ShinkiEmuCONTROL_RESUME:ShinkiEmuCONTROL_PAUSE,&error));
        } else if(e.kind==2 && width>0 && height>0) {
            const auto sw=game.surface.raster.width, sh=game.surface.raster.height;
            const double scale=std::min(double(width)/sw,double(height)/sh);
            ShinkiEmuInputEvent input{};
            input.displayed=last;input.x=(e.x-(width-sw*scale)/2)/scale;input.y=(e.y-(height-sh*scale)/2)/scale;
            input.kind=e.text=="move"?ShinkiEmuINPUT_POINTER_MOVE:ShinkiEmuINPUT_POINTER_BUTTON;
            input.button=ShinkiEmuBUTTON_PRIMARY;input.pressed=e.text=="down";
            check(api->input(game.session,&input,1,&error));
        } else if(e.kind==3 && e.x==2 && e.y==0) {
            close();shinki_ohos_notify("closed","");return;
        }
        if(foreground) shinki_ohos_request_frame();
    }
    void loop() {
        for(;;) {
            Event e;
            {std::unique_lock lock(mutex);wake.wait(lock,[&]{return exiting||!queue.empty();});
             if(exiting) break;e=std::move(queue.front());queue.pop_front();}
            try {dispatch(e);} catch(const std::exception &ex) {close();shinki_ohos_notify("error",ex.what());}
        }
        close();
    }
public:
    Player():worker([this]{loop();}) {}
    ~Player() {{std::lock_guard lock(mutex);exiting=true;}wake.notify_one();worker.join();}
    void send(Event e) {
        std::lock_guard lock(mutex);
        if (!queue.empty() && e.kind==2 && e.text=="move" && queue.back().kind==2 && queue.back().text=="move") queue.back()=std::move(e);
        else queue.push_back(std::move(e));
        wake.notify_one();
    }
};
Player &player() {static Player value;return value;}
}
extern "C" void shinki_ohos_event(int kind,double x,double y,const char *text){player().send({kind,x,y,text?text:""});}
extern "C" void shinki_ohos_start(const char *selection){player().send({8,0,0,selection});}
extern "C" void shinki_ohos_pick_result(uint32_t,const char *,const char *){}
