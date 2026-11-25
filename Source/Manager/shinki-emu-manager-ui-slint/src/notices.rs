use std::{cell::RefCell, collections::HashSet};

use slint::{ComponentHandle, Model, ModelTracker, VecModel};

use crate::{ManagerWindow, Notice, NoticeViewModel, Notices, NoticesViewModel, tr};

pub struct ErrorText {
    pub title: String,
    pub detail: String,
    pub code: String,
}

pub fn describe_error(raw: &str) -> ErrorText {
    let (code, rest) = split_code(raw);
    if code.is_empty() {
        let mut lines = raw.split('\n');
        let title = lines.next().unwrap_or("").to_string();
        let detail = lines.collect::<Vec<_>>().join("\n");
        return ErrorText {
            title,
            detail,
            code: String::new(),
        };
    }
    if let Some((title, detail)) = exact_text(&code) {
        return ErrorText {
            title: title.to_string(),
            detail: detail.to_string(),
            code,
        };
    }
    ErrorText {
        title: fallback_title(&code).to_string(),
        detail: rest,
        code,
    }
}

fn split_code(raw: &str) -> (String, String) {
    let token = raw
        .split(|c: char| c.is_whitespace() || c == ':')
        .next()
        .unwrap_or("");
    if !is_error_code(token) {
        return (String::new(), String::new());
    }
    let rest = raw
        .get(token.len()..)
        .unwrap_or("")
        .strip_prefix(": ")
        .unwrap_or("")
        .to_string();
    (token.to_string(), rest)
}

fn is_error_code(token: &str) -> bool {
    let Some(rest) = token.strip_prefix("SHINKI_EMU_") else {
        return false;
    };
    !rest.is_empty()
        && rest
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

fn exact_text(code: &str) -> Option<(&'static str, &'static str)> {
    Some(match code {
        "SHINKI_EMU_CORES_NOT_READY" => {
            (tr!("模拟器核心尚未加载"), tr!("请先在游戏库点击“准备”。"))
        }
        "SHINKI_EMU_FAMILY_INITIALIZING" => (tr!("核心仍在加载或扫描中"), tr!("请稍候再试。")),
        "SHINKI_EMU_PREPARE_BUSY" => (
            tr!("正在准备资料库"),
            tr!("请等待当前的核心加载或扫描结束。"),
        ),
        "SHINKI_EMU_PREPARE_NOT_CANCELLABLE" => {
            (tr!("当前没有可取消的扫描"), tr!("核心加载阶段不能取消。"))
        }
        "SHINKI_EMU_PREPARE_RESTART_REQUIRED" => (
            tr!("资料库准备失败"),
            tr!("请查看核心诊断，修复后重启 ShinkiEMU。"),
        ),
        "SHINKI_EMU_SCAN_CANCELLED" => (tr!("已取消扫描"), tr!("资料库保持不变。")),
        "SHINKI_EMU_GAME_DIRECTORY_INVALID" => {
            (tr!("无法使用这个目录"), tr!("目录不存在或无法访问。"))
        }
        "SHINKI_EMU_UNDO_UNAVAILABLE" => (
            tr!("没有可撤销的移除"),
            tr!("作品可以在 设置→库目录 中恢复。"),
        ),
        "SHINKI_EMU_OPEN_PATH_FAILED" => (tr!("无法打开所在目录"), tr!("系统文件管理器未能启动。")),
        "SHINKI_EMU_KEY_CAPTURE_INACTIVE" => {
            (tr!("没有正在设置的按键"), tr!("请先点选要绑定的手柄按钮。"))
        }
        "SHINKI_EMU_PATCH_RECIPE_UNKNOWN" => (
            tr!("这个补丁不适用于所选游戏"),
            tr!("补丁列表可能已随元数据同步或模拟器选择更新，请重新选择。"),
        ),
        "SHINKI_EMU_PATCH_GAME_REQUIRED" => (
            tr!("请先选择游戏"),
            tr!("在补丁页的第一步选择要转换的游戏。"),
        ),
        "SHINKI_EMU_GAMEPAD_BUTTON_INVALID" => {
            (tr!("无法识别的手柄按钮"), tr!("请从按键列表中选择。"))
        }
        "SHINKI_EMU_FAMILY_SESSION_ALREADY_ACTIVE" => {
            (tr!("已有游戏正在运行"), tr!("请先返回游戏库结束当前游戏。"))
        }
        "SHINKI_EMU_SESSION_CLOSING" => (
            tr!("游戏正在关闭，请稍候"),
            tr!("核心释放完成后即可继续操作。"),
        ),
        "SHINKI_EMU_FAMILY_PROBE_REQUIRED" => {
            (tr!("这部作品尚未识别"), tr!("请重新扫描游戏目录。"))
        }
        "SHINKI_EMU_FAMILY_PROVIDER_SELECTION_REQUIRED" => (
            tr!("需要选择模拟器"),
            tr!("有多个模拟器可以运行这部作品，请在作品详情中选择一个。"),
        ),
        "SHINKI_EMU_GAME_NOT_FOUND" => {
            (tr!("找不到这部作品"), tr!("它可能已被移除，请刷新资料库。"))
        }
        "SHINKI_EMU_OWNER_ALREADY_STARTED" => {
            (tr!("核心已加载"), tr!("更换核心后需要重启 ShinkiEMU。"))
        }
        "SHINKI_EMU_OWNER_PANICKED" => (
            tr!("核心后台意外退出"),
            tr!("请重启 ShinkiEMU 并查看日志。"),
        ),
        "SHINKI_EMU_FAMILY_LOAD_LIBRARY" => (
            tr!("无法加载核心文件"),
            tr!("文件可能已损坏，或缺少它依赖的运行库。"),
        ),
        "SHINKI_EMU_FAMILY_LOAD_ABI" => (
            tr!("核心版本与 ShinkiEMU 不兼容"),
            tr!("请安装与当前版本匹配的核心。"),
        ),
        "SHINKI_EMU_FAMILY_LOAD_DESCRIPTOR" => (tr!("核心信息无效"), tr!("核心文件可能已损坏。")),
        "SHINKI_EMU_FAMILY_LOAD_DUPLICATE_PLUGIN" => (
            tr!("发现重复的核心"),
            tr!("同一个核心存在多份文件，已全部停用。删除多余的一份后重启。"),
        ),
        "SHINKI_EMU_FAMILY_LOAD_PATH" => {
            (tr!("核心文件位置无效"), tr!("请检查核心文件是否仍然存在。"))
        }
        "SHINKI_EMU_FAMILY_LOAD_POLICY" => (tr!("核心不符合安全要求"), tr!("已停用这个核心。")),
        "SHINKI_EMU_FAMILY_DIAGNOSTIC_INIT" => (tr!("核心诊断初始化失败"), tr!("请查看日志。")),
        "SHINKI_EMU_FAMILY_SCAN_DIRECTORY" => (tr!("无法读取核心目录"), tr!("请检查目录权限。")),
        "SHINKI_EMU_CORE_RETRY_UNAVAILABLE" => {
            (tr!("无法重试"), tr!("这个错误不支持重试，请结束游戏。"))
        }
        "SHINKI_EMU_INPUT_QUEUE_FULL" => (
            tr!("输入过多"),
            tr!("游戏未能及时处理输入，多余的输入已丢弃。"),
        ),
        "SHINKI_EMU_RENDER_PREFERENCE_INVALID" => (
            tr!("渲染分辨率设置无效"),
            tr!("预设只能选 720p、1080p、1440p 或 2160p，自定义宽高须在 1 到 16384 之间。"),
        ),
        "SHINKI_EMU_RENDER_EXTENT_UNSUPPORTED" => (
            tr!("自定义分辨率超出该模拟器支持范围"),
            tr!("请在游戏设置中查看该模拟器支持的分辨率范围。"),
        ),
        "SHINKI_EMU_EMULATOR_SELECTION_REQUIRED" => (
            tr!("需要先选择模拟器"),
            tr!("选定模拟器后才能修改这部作品的模拟器设置。"),
        ),
        "SHINKI_EMU_CONFIG_PATH_INVALID" => {
            (tr!("无法使用所选路径"), tr!("文件或目录不存在或无法访问。"))
        }
        "SHINKI_EMU_CONFIG_PATH_UTF8" => (
            tr!("所选路径包含无法保存的字符"),
            tr!("请把文件放到路径只含常规字符的位置后重新选择。"),
        ),
        "SHINKI_EMU_FAMILY_CONFIG" => (
            tr!("设置值无效"),
            tr!("取值不在允许范围内，已保留原来的设置。"),
        ),
        "SHINKI_EMU_RELEASE_ID_INVALID" => (
            tr!("发行版本编号无效"),
            tr!("VNDB 发行版本编号应为 r 加数字，例如 r12345，且不超过 64 个字符。"),
        ),
        "SHINKI_EMU_COMPATIBILITY_DUPLICATE" => (
            tr!("兼容性数据有重复条目"),
            tr!("同一发行版本和模拟器出现了多条记录，本次更新未采用。"),
        ),
        "SHINKI_EMU_TRANSLATION_PROFILE_REQUIRED" => (
            tr!("尚未配置翻译服务"),
            tr!("请在 设置→实时翻译 中填写并保存。"),
        ),
        "SHINKI_EMU_NETWORK_PERMISSION_REQUIRED" => (
            tr!("尚未允许联网"),
            tr!("需要先在 设置→联网与资料 中允许联网"),
        ),
        "SHINKI_EMU_BANGUMI_TOKEN_REQUIRED" => {
            (tr!("请填写 Bangumi 访问令牌"), tr!("令牌为空时无法保存。"))
        }
        "SHINKI_EMU_CORE_INSTALL_NAME" => (
            tr!("不是有效的核心文件"),
            tr!("请选择核心包（.zip）或文件名以 shinki_emu_ 开头的本系统动态库。"),
        ),
        "SHINKI_EMU_CORE_INSTALL_COPY" => {
            (tr!("复制核心文件失败"), tr!("请检查磁盘空间和目录权限。"))
        }
        "SHINKI_EMU_CORE_UNINSTALL_BUNDLED" => {
            (tr!("随包核心不能卸载"), tr!("可以安装同名核心来覆盖它。"))
        }
        "SHINKI_EMU_CORE_PENDING_APPLY" => (tr!("应用待安装的核心失败"), tr!("请查看日志后重试。")),
        "SHINKI_EMU_CORE_LAYOUT" => (
            tr!("核心目录结构不正确"),
            tr!("cores 下每个核心目录必须恰好包含一个 shinki_emu_ 核心库，请重新安装该核心。"),
        ),
        "SHINKI_EMU_CORE_PACKAGE_READ" => {
            (tr!("无法读取核心包"), tr!("请检查文件是否存在且可以访问。"))
        }
        "SHINKI_EMU_CORE_PACKAGE_FORMAT" => {
            (tr!("核心包已损坏"), tr!("文件不是有效的 zip 核心包。"))
        }
        "SHINKI_EMU_CORE_PACKAGE_BOUNDS" => (
            tr!("核心包过大"),
            tr!("核心包的文件数或解压后大小超出上限。"),
        ),
        "SHINKI_EMU_CORE_PACKAGE_PATH" => (
            tr!("核心包包含不安全的路径"),
            tr!("包内有绝对路径、上级目录或符号链接，已拒绝安装。"),
        ),
        "SHINKI_EMU_CORE_PACKAGE_DUPLICATE" => {
            (tr!("核心包包含重复文件"), tr!("已拒绝安装这个核心包。"))
        }
        "SHINKI_EMU_CORE_PACKAGE_MANIFEST" => (
            tr!("核心包的文件清单无效"),
            tr!("core.json 缺失、格式错误、ABI 不匹配或与包内文件不一致。"),
        ),
        "SHINKI_EMU_CORE_PACKAGE_PLUGIN_MISMATCH" => (
            tr!("核心包与所选核心不符"),
            tr!("下载的核心包声明了另一个模拟器，已拒绝安装。"),
        ),
        "SHINKI_EMU_CORE_PACKAGE_HASH_MISMATCH" => (
            tr!("核心包文件校验失败"),
            tr!("包内文件的 SHA-256 与 core.json 不一致，已拒绝安装。"),
        ),
        "SHINKI_EMU_CORE_NOT_AVAILABLE" => (
            tr!("没有可安装的核心版本"),
            tr!("元数据中没有适用于当前平台和接口版本的发布，请先同步元数据。"),
        ),
        "SHINKI_EMU_CORE_DOWNLOAD_PENDING" => (tr!("核心正在下载"), tr!("请等待当前下载完成。")),
        "SHINKI_EMU_CORE_DOWNLOAD_URL" => (
            tr!("核心下载地址无效"),
            tr!("只允许从 GitHub Releases 下载核心。"),
        ),
        "SHINKI_EMU_CORE_DOWNLOAD_REDIRECT" => (
            tr!("核心下载被重定向到未知地址"),
            tr!("只允许跳转到 GitHub 的发布文件服务器，已停止下载。"),
        ),
        "SHINKI_EMU_CORE_DOWNLOAD_NETWORK" => (tr!("核心下载失败"), tr!("请检查网络连接后重试。")),
        "SHINKI_EMU_CORE_DOWNLOAD_SIZE" => (
            tr!("核心包大小不符"),
            tr!("下载的文件大小与元数据记录不一致，已丢弃。"),
        ),
        "SHINKI_EMU_CORE_DOWNLOAD_HASH_MISMATCH" => (
            tr!("核心包校验失败"),
            tr!("下载文件的 SHA-256 与元数据记录不一致，已丢弃。"),
        ),
        "SHINKI_EMU_SNACKBAR_ACTION_UNAVAILABLE" => {
            (tr!("这个操作已失效"), tr!("提示已过期，请重新操作。"))
        }
        "SHINKI_EMU_METADATA_HASH_MISMATCH" => (
            tr!("元数据校验失败"),
            tr!("下载的文件与索引记录不一致，已保留原有数据。"),
        ),
        "SHINKI_EMU_METADATA_CACHE_WRITE" => (
            tr!("无法保存元数据"),
            tr!("写入数据目录失败，已保留原有数据。请检查磁盘空间和目录权限。"),
        ),
        "SHINKI_EMU_METADATA_CACHE_READ" => {
            (tr!("无法读取元数据缓存"), tr!("请检查数据目录权限。"))
        }
        "SHINKI_EMU_METADATA_CACHE_INVALID" => (
            tr!("元数据内容无效"),
            tr!("同步到的数据无法解析，已保留原有数据。"),
        ),
        "SHINKI_EMU_METADATA_REQUEST_REJECTED" => (
            tr!("作品资料服务拒绝了请求"),
            tr!("VNDB 不接受这次查询。请更新 ShinkiEMU 后重试，仍失败请反馈问题。"),
        ),
        "SHINKI_EMU_METADATA_RATE_LIMITED" => (
            tr!("作品资料请求过于频繁"),
            tr!("VNDB 或 Bangumi 暂时限制了请求，请稍后再试。"),
        ),
        "SHINKI_EMU_PRESET_INVALID" => (
            tr!("推荐配置无法套用"),
            tr!("其中有当前模拟器不支持的设置项或取值，已恢复原来的设置。"),
        ),
        "SHINKI_EMU_PRESET_UNAVAILABLE" => (
            tr!("没有推荐配置"),
            tr!("当前发行版本和模拟器没有推荐配置。"),
        ),
        "SHINKI_EMU_LOG_READ" => (tr!("无法读取日志"), tr!("请检查数据目录权限。")),
        "SHINKI_EMU_ENGINE_ACTION_UNAVAILABLE" => {
            (tr!("当前无法执行这个操作"), tr!("请在游戏画面中稍后再试。"))
        }
        _ => return None,
    })
}

fn fallback_title(code: &str) -> &'static str {
    let rest = code.strip_prefix("SHINKI_EMU_").unwrap_or(code);
    match rest.split('_').next().unwrap_or("") {
        "FAMILY" | "CORE" | "CORES" | "MODULE" | "OWNER" | "PROBE" | "EMULATOR" | "STATIC"
        | "MUSICA" | "FVP" | "SESSION" => tr!("模拟器核心出错"),
        "HOST" | "WINDOW" | "GPU" | "FRAME" | "SLINT" | "SYSTEM" | "OUT" | "RENDER" => {
            tr!("窗口或显示出错")
        }
        "TEXT" | "TRANSLATION" | "EXTENSION" => tr!("实时翻译出错"),
        "METADATA" | "VNDB" | "BANGUMI" | "NETWORK" | "RELEASE" => tr!("作品资料服务出错"),
        "COMPATIBILITY" => tr!("兼容性数据出错"),
        "FILTER" | "EFFECT" | "DXC" => tr!("画面滤镜出错"),
        "PATCH" => tr!("文件补丁处理失败"),
        "INPUT" | "GAMEPAD" => tr!("输入设备出错"),
        "LIBRARY" | "GAME" | "CASE" | "SEARCH" | "DATA" | "EXECUTABLE" | "PREPARE" | "SCAN"
        | "OPEN" => tr!("游戏库出错"),
        "PRESET" => tr!("推荐配置出错"),
        "AUDIO" => tr!("声音输出出错"),
        "MODAL" | "MANAGER" => tr!("管理器出错"),
        "WEB" => tr!("网页登录出错"),
        _ => tr!("发生错误"),
    }
}

pub(crate) struct NoticeModel {
    rows: VecModel<Notice>,
    core_inventory_error: RefCell<Option<NoticeViewModel>>,
    model_items: RefCell<Vec<NoticeViewModel>>,
    host_items: RefCell<Vec<NoticeViewModel>>,
    dismissed: RefCell<HashSet<String>>,
}

impl NoticeModel {
    pub(crate) fn new() -> Self {
        Self {
            rows: VecModel::default(),
            core_inventory_error: RefCell::new(None),
            model_items: RefCell::new(Vec::new()),
            host_items: RefCell::new(Vec::new()),
            dismissed: RefCell::new(HashSet::new()),
        }
    }

    pub(crate) fn set_model(&self, view: &NoticesViewModel) {
        let mut dismissed = self.dismissed.borrow_mut();
        let host = self.host_items.borrow();
        dismissed.retain(|id| {
            view.core_inventory_error
                .iter()
                .chain(&view.items)
                .chain(host.iter())
                .any(|item| item.id == *id)
        });
        drop(host);
        self.core_inventory_error
            .borrow_mut()
            .clone_from(&view.core_inventory_error);
        self.model_items.borrow_mut().clone_from(&view.items);
        drop(dismissed);
        self.rebuild();
    }

    pub(crate) fn report(&self, raw: &str) {
        let text = describe_error(raw);
        let id = format!(
            "host:{}",
            if text.code.is_empty() {
                text.title.as_str()
            } else {
                text.code.as_str()
            }
        );
        let mut host = self.host_items.borrow_mut();
        host.retain(|item| item.id != id);
        host.insert(
            0,
            NoticeViewModel {
                id: id.clone(),
                severity: "error".into(),
                title: text.title,
                detail: text.detail,
                code: text.code,
                core: String::new(),
            },
        );
        host.truncate(5);
        drop(host);
        self.dismissed.borrow_mut().remove(&id);
        self.rebuild();
    }

    pub(crate) fn dismiss(&self, id: &str) {
        self.dismissed.borrow_mut().insert(id.to_string());
        self.host_items.borrow_mut().retain(|item| item.id != id);
        self.rebuild();
    }

    fn rebuild(&self) {
        let dismissed = self.dismissed.borrow();
        let mut rows: Vec<_> = self
            .host_items
            .borrow()
            .iter()
            .chain(self.model_items.borrow().iter())
            .filter(|item| !dismissed.contains(&item.id))
            .map(to_notice)
            .collect();
        if let Some(item) = self.core_inventory_error.borrow().as_ref()
            && !dismissed.contains(&item.id)
        {
            // NoticeStack shows three cards. Keep this one low in short windows.
            rows.insert(rows.len().min(2), to_notice(item));
        }
        self.rows.set_vec(rows);
    }
}

fn to_notice(item: &NoticeViewModel) -> Notice {
    Notice {
        id: item.id.as_str().into(),
        severity: item.severity.as_str().into(),
        title: item.title.as_str().into(),
        detail: item.detail.as_str().into(),
        code: item.code.as_str().into(),
        core: item.core.as_str().into(),
    }
}

impl Model for NoticeModel {
    type Data = Notice;
    fn row_count(&self) -> usize {
        self.rows.row_count()
    }
    fn row_data(&self, row: usize) -> Option<Notice> {
        self.rows.row_data(row)
    }
    fn model_tracker(&self) -> &dyn ModelTracker {
        self.rows.model_tracker()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub fn report_error(window: &ManagerWindow, raw: &str) {
    let items = window.global::<Notices>().get_items();
    if let Some(model) = items.as_any().downcast_ref::<NoticeModel>() {
        model.report(raw);
        return;
    }
    tracing::error!(event = "shinki.emu.manager.notice_model_missing", raw);
}

#[cfg(test)]
mod tests {
    use super::describe_error;

    #[test]
    fn describe_error_falls_back_by_family_token() {
        let text = describe_error("SHINKI_EMU_FILTER_FOO: bad");
        assert_eq!(text.detail, "bad");
        assert_eq!(text.code, "SHINKI_EMU_FILTER_FOO");
    }

    #[test]
    fn describe_error_uses_first_line_without_code() {
        let text = describe_error("扫描失败\n原因");
        assert_eq!(text.title, "扫描失败");
        assert_eq!(text.detail, "原因");
        assert_eq!(text.code, "");
    }
}
