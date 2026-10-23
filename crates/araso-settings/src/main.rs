//! Araso's settings window: the settings (layouts, keys), the rules for apps and the words Araso
//! learned, as settings.json, apps.json and dictionary.json in %APPDATA%\Araso keep them. Every
//! change is saved at once; the input method reads the files again within two seconds.

#![windows_subsystem = "windows"]

mod apps;

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use araso_core::app_policy::{self, STRICT_THRESHOLD};
use araso_core::settings::{self, PauseKey, Tap};
use araso_core::support_dir;
use araso_core::user_dictionary::{self, Entry, Source, Strength, UserDictionary};
use araso_core::{AppPolicies, AppPolicy, KoreanLayout, Language, LatinLayout, Settings, SwitchKey};
use eframe::egui;

const TITLE: &str = "Araso 설정";
/// This port's home.
const HOME: &str = "https://github.com/mununki/araso-windows";
/// Upstream: the original Araso, for macOS.
const UPSTREAM: &str = "https://nyttastudio.com/araso";
/// Where to ask about or report a problem with this one.
const ISSUES: &str = "https://github.com/mununki/araso-windows/issues";

fn main() -> eframe::Result {
    // Opened again (the taskbar menu, twice): the window already open comes forward.
    if apps::bring_forward(TITLE) {
        return Ok(());
    }
    // The tab to open on: araso-settings.exe [general|apps|words|about].
    let tab = match std::env::args().nth(1).as_deref() {
        Some("apps") => Tab::Apps,
        Some("words") => Tab::Words,
        Some("about") => Tab::About,
        _ => Tab::General,
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(TITLE)
        .with_inner_size([600.0, 640.0])
        .with_min_inner_size([480.0, 420.0]);
    if let Some(icon) = icon() {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        TITLE,
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_fonts(fonts());
            // Scroll bars of their own width, not thin ones that show only under the mouse.
            cc.egui_ctx.all_styles_mut(|style| style.spacing.scroll = egui::style::ScrollStyle::solid());
            let mut app = SettingsApp::load();
            app.tab = tab;
            Ok(Box::new(app))
        }),
    )
}

/// Araso's icon for the title bar: the 48-pixel PNG in araso.ico.
fn icon() -> Option<egui::IconData> {
    let ico: &[u8] = include_bytes!("../../araso-tsf/res/araso.ico");
    let count = u16::from_le_bytes([*ico.get(4)?, *ico.get(5)?]) as usize;
    (0..count).find_map(|i| {
        let entry = ico.get(6 + 16 * i..6 + 16 * (i + 1))?;
        let length = u32::from_le_bytes(entry[8..12].try_into().ok()?) as usize;
        let offset = u32::from_le_bytes(entry[12..16].try_into().ok()?) as usize;
        let png = ico.get(offset..offset + length)?;
        (entry[0] == 48).then(|| eframe::icon_data::from_png_bytes(png).ok()).flatten()
    })
}

/// egui's fonts, with Malgun Gothic (Windows' Korean font) first: egui's own have no Hangul.
fn fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    let windows = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let path = std::path::Path::new(&windows).join("Fonts").join("malgun.ttf");
    if let Ok(data) = std::fs::read(path) {
        fonts.font_data.insert("malgun".into(), Arc::new(egui::FontData::from_owned(data)));
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "malgun".into());
        fonts.families.entry(egui::FontFamily::Monospace).or_default().push("malgun".into());
    }
    fonts
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    General,
    Apps,
    Words,
    About,
}

struct SettingsApp {
    tab: Tab,
    settings: Settings,
    policies: AppPolicies,
    dictionary: UserDictionary,
    /// When dictionary.json was last read or written here: the input method writes it too.
    dictionary_stamp: Option<SystemTime>,
    checked: Instant,
    new_app: String,
    running: Vec<String>,
    new_keys: String,
    new_language: Language,
    filter: String,
    confirm_forget: bool,
    status: Option<String>,
}

const SWITCH_KEYS: [(SwitchKey, &str); 5] = [
    (SwitchKey::HangulKey, "한/영 키"),
    (SwitchKey::RightAlt, "오른쪽 Alt (혼자 눌렀다 떼기)"),
    (SwitchKey::RightShift, "오른쪽 Shift (혼자 눌렀다 떼기)"),
    (SwitchKey::Escape, "Esc"),
    (SwitchKey::ShiftEscape, "Shift+Esc"),
];

const PAUSE_KEYS: [(PauseKey, &str); 5] = [
    (PauseKey::Tap(Tap::HanjaKey), "한자 키"),
    (PauseKey::Tap(Tap::RightControl), "오른쪽 Ctrl (혼자 눌렀다 떼기)"),
    (PauseKey::Tap(Tap::RightAlt), "오른쪽 Alt (혼자 눌렀다 떼기)"),
    (PauseKey::Tap(Tap::RightShift), "오른쪽 Shift (혼자 눌렀다 떼기)"),
    (PauseKey::Off, "쓰지 않음"),
];

const LANGUAGES: [(Language, &str); 3] =
    [(Language::English, "영어"), (Language::Korean, "한국어"), (Language::Mixed, "섞어 씀 (github에)")];

fn language_label(language: Language) -> &'static str {
    match language {
        Language::English => "영어",
        Language::Korean => "한국어",
        Language::Mixed => "섞어 씀",
        Language::Japanese => "일본어",
    }
}

fn read_dictionary() -> (UserDictionary, Option<SystemTime>) {
    let Some(url) = user_dictionary::default_store_url() else { return (UserDictionary::default(), None) };
    let dictionary = std::fs::read(&url).ok().and_then(|data| UserDictionary::decode(&data)).unwrap_or_default();
    (dictionary, support_dir::modified(&url))
}

/// A scrolling area whose scroll bar is always there: what's below the window's edge doesn't
/// hide.
fn scroll_area() -> egui::ScrollArea {
    egui::ScrollArea::vertical().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
}

/// The section's title, and what it is for.
fn section(ui: &mut egui::Ui, title: &str, hint: &str) {
    ui.add_space(14.0);
    ui.heading(title);
    if !hint.is_empty() {
        ui.label(egui::RichText::new(hint).weak());
    }
    ui.add_space(4.0);
}

impl SettingsApp {
    fn load() -> Self {
        let (dictionary, dictionary_stamp) = read_dictionary();
        SettingsApp {
            tab: Tab::General,
            settings: Settings::load_default(),
            policies: AppPolicies::load_default(),
            dictionary,
            dictionary_stamp,
            checked: Instant::now(),
            new_app: String::new(),
            running: apps::running(),
            new_keys: String::new(),
            new_language: Language::Korean,
            filter: String::new(),
            confirm_forget: false,
            status: None,
        }
    }

    fn report(&mut self, what: &str, result: std::io::Result<()>) {
        self.status = match result {
            Ok(()) => None,
            Err(e) => Some(format!("{what}을(를) 저장하지 못했습니다: {e}")),
        };
    }

    fn save_settings(&mut self) {
        let _ = support_dir::url().map(std::fs::create_dir_all);
        let result = self.settings.save(settings::store_url().as_deref());
        self.report("설정", result);
    }

    fn save_policies(&mut self) {
        let _ = support_dir::url().map(std::fs::create_dir_all);
        let result = self.policies.save(app_policy::store_url().as_deref());
        self.report("앱별 규칙", result);
    }

    fn save_dictionary(&mut self) {
        let Some(url) = user_dictionary::default_store_url() else { return };
        let _ = support_dir::url().map(std::fs::create_dir_all);
        let result = match self.dictionary.encoded() {
            Some(data) => support_dir::write_atomically(&url, &data),
            None => Ok(()),
        };
        self.report("학습한 단어", result);
        self.dictionary_stamp = support_dir::modified(&url);
    }

    /// Words the input method learned meanwhile show up.
    fn reread_dictionary(&mut self) {
        if self.checked.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.checked = Instant::now();
        let stamp = user_dictionary::default_store_url().and_then(|url| support_dir::modified(&url));
        if stamp != self.dictionary_stamp {
            (self.dictionary, self.dictionary_stamp) = read_dictionary();
        }
    }

    fn general(&mut self, ui: &mut egui::Ui) {
        let before = self.settings.clone();
        let mut policy = self.policies.default_policy();
        let policy_before = policy.clone();

        section(ui, "한글 자판", "");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.settings.korean_layout, KoreanLayout::Dubeolsik, "두벌식");
            ui.radio_value(&mut self.settings.korean_layout, KoreanLayout::SebeolsikFinal, "세벌식 최종");
        });

        section(ui, "영문 배열", "영어(와 로마자)를 치는 배열입니다. 두벌식 한글은 언제나 제자리에 있습니다.");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.settings.latin_layout, LatinLayout::Qwerty, "QWERTY");
            ui.radio_value(&mut self.settings.latin_layout, LatinLayout::Colemak, "Colemak");
            ui.radio_value(&mut self.settings.latin_layout, LatinLayout::ColemakDH, "Colemak-DH");
        });

        section(
            ui,
            "언어 바꾸기 키",
            "치고 있는 단어나 방금 확정한 단어를 다른 언어로 바꿉니다. 같은 쪽으로 두 번 바꾸면 그 단어를 익힙니다.",
        );
        for (key, label) in SWITCH_KEYS {
            let mut on = policy.switch_keys.contains(&key);
            if ui.checkbox(&mut on, label).changed() {
                if on {
                    policy.switch_keys.insert(key);
                } else {
                    policy.switch_keys.remove(&key);
                }
            }
        }

        section(
            ui,
            "일시정지 키",
            "이 입력창에서 Araso를 잠시 끕니다(친 그대로 들어갑니다). 작업 표시줄의 아이콘을 눌러도 됩니다.",
        );
        if let PauseKey::Shortcut(shortcut) = &self.settings.pause_key {
            ui.label(format!("단축키 {} (settings.json에서 정함)", shortcut.label()));
        }
        for (key, label) in PAUSE_KEYS {
            ui.radio_value(&mut self.settings.pause_key, key, label);
        }

        section(ui, "단어", "");
        ui.checkbox(&mut self.settings.return_confirms_only, "Enter는 밑줄 친 단어를 확정만 하기 (앱에는 보내지 않음)");
        let mut strict = policy.is_strict();
        if ui.checkbox(&mut strict, "엄격하게: 아주 확실할 때만 바꾸기 (짧은 명령어가 많은 곳)").changed() {
            policy.conversion_threshold =
                if strict { STRICT_THRESHOLD } else { AppPolicy::standard().conversion_threshold };
        }
        ui.checkbox(&mut policy.hold_space, "Space 뒤에도 단어를 잡아 두기 (확정한 단어를 고칠 수 없는 앱에서)");

        section(ui, "일본어 (실험)", "로마자로 친 일본어를 가나로 바꿉니다. 한자 변환은 아직 없습니다.");
        ui.checkbox(&mut self.settings.japanese, "일본어도 알아보기");
        if self.settings.japanese {
            ui.checkbox(&mut self.settings.full_width_digits, "일본어 뒤의 숫자를 전각으로 (１２３)");
            ui.checkbox(&mut self.settings.full_width_space, "일본어 뒤의 공백을 전각으로");
        }

        if self.settings != before {
            self.save_settings();
        }
        if policy != policy_before {
            self.policies.set_default(&policy);
            self.save_policies();
        }
    }

    fn apps(&mut self, ui: &mut egui::Ui) {
        section(
            ui,
            "앱별 규칙",
            "실행 파일 이름으로 앱마다 따로 정합니다. 정하지 않은 것은 일반 설정을 따릅니다. 작업 표시줄 아이콘의 메뉴에서도 앱을 끌 수 있습니다.",
        );
        let mut changed = false;
        let mut remove = None;
        let apps = self.policies.apps();
        if apps.is_empty() {
            ui.label(egui::RichText::new("아직 없습니다.").weak());
        }
        for app in &apps {
            let mut policy = self.policies.policy(Some(app));
            let before = policy.clone();
            ui.horizontal(|ui| {
                ui.strong(app);
                ui.checkbox(&mut policy.enabled, "쓰기");
                let mut strict = policy.is_strict();
                if ui.checkbox(&mut strict, "엄격하게").changed() {
                    policy.conversion_threshold =
                        if strict { STRICT_THRESHOLD } else { AppPolicy::standard().conversion_threshold };
                }
                ui.checkbox(&mut policy.hold_space, "Space 뒤에도 잡아 두기");
                if ui.button("빼기").clicked() {
                    remove = Some(app.clone());
                }
            });
            if policy != before {
                self.policies.set(&policy, app);
                changed = true;
            }
        }
        if let Some(app) = remove {
            self.policies.overrides.remove(&app);
            changed = true;
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label("앱 더하기");
            ui.add(egui::TextEdit::singleline(&mut self.new_app).hint_text("예: windowsterminal.exe").desired_width(200.0));
            egui::ComboBox::from_id_salt("running").selected_text("열린 앱에서 고르기").show_ui(ui, |ui| {
                for name in &self.running {
                    ui.selectable_value(&mut self.new_app, name.clone(), name);
                }
            });
            let name = self.new_app.trim().to_lowercase();
            if ui.add_enabled(!name.is_empty(), egui::Button::new("더하기")).clicked() {
                let policy = self.policies.policy(Some(&name));
                self.policies.set(&policy, &name);
                self.new_app.clear();
                changed = true;
            }
        });

        if changed {
            self.save_policies();
        }
    }

    fn words(&mut self, ui: &mut egui::Ui) {
        section(
            ui,
            "학습한 단어",
            "언어를 바꾼 단어는 한 번이면 그쪽으로 기울고(30일 안에 다시 쓰지 않으면 잊음), 두 번이면 확실히 익힙니다. 직접 더할 수도 있습니다.",
        );
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("더하기");
            ui.add(egui::TextEdit::singleline(&mut self.new_keys).hint_text("친 글자 (예: dkssud)").desired_width(150.0));
            egui::ComboBox::from_id_salt("language").selected_text(language_label(self.new_language)).show_ui(ui, |ui| {
                for (language, label) in LANGUAGES {
                    ui.selectable_value(&mut self.new_language, language, label);
                }
            });
            let keys = self.new_keys.trim().to_string();
            if !keys.is_empty() {
                ui.label(format!("→ {}", UserDictionary::reading(&keys, self.new_language)));
            }
            if ui.add_enabled(!keys.is_empty(), egui::Button::new("더하기")).clicked() {
                self.dictionary.add(Entry::user(&keys, self.new_language));
                self.new_keys.clear();
                changed = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("찾기");
            ui.add(egui::TextEdit::singleline(&mut self.filter).desired_width(150.0));
        });
        ui.separator();

        let today = user_dictionary::local_today();
        let filter = self.filter.trim().to_lowercase();
        let mut remove = None;
        scroll_area().max_height(ui.available_height() - 40.0).show(ui, |ui| {
            let entries = self.dictionary.entries();
            if entries.is_empty() {
                ui.label(egui::RichText::new("아직 없습니다.").weak());
            }
            for entry in entries {
                let reading = entry
                    .text
                    .clone()
                    .or_else(|| entry.language.map(|l| UserDictionary::reading(&entry.keys, l)))
                    .unwrap_or_default();
                if !filter.is_empty() && !entry.keys.to_lowercase().contains(&filter) && !reading.contains(&filter) {
                    continue;
                }
                let how = match (entry.source, entry.strength) {
                    (Source::User, _) => "직접 더함".to_string(),
                    (Source::Learned, Some(Strength::Sure)) => "익힘".to_string(),
                    _ => match UserDictionary::days_left(entry, today) {
                        Some(days) => format!("기우는 중 ({days}일 남음)"),
                        None => "기우는 중".to_string(),
                    },
                };
                ui.horizontal(|ui| {
                    ui.monospace(&entry.keys);
                    ui.label("→");
                    ui.strong(&reading);
                    if let Some(language) = entry.language {
                        ui.label(language_label(language));
                    }
                    ui.label(egui::RichText::new(how).weak());
                    if let Some(apps) = &entry.apps {
                        ui.label(egui::RichText::new(apps.join(", ")).weak());
                    }
                    if ui.small_button("지우기").clicked() {
                        remove = Some(entry.id());
                    }
                });
            }
        });
        if let Some(id) = remove {
            self.dictionary.remove(&id);
            changed = true;
        }

        ui.horizontal(|ui| {
            if self.confirm_forget {
                ui.label("학습한 단어를 모두 잊을까요? (직접 더한 것은 남습니다)");
                if ui.button("모두 잊기").clicked() {
                    self.dictionary.forget_learned();
                    self.confirm_forget = false;
                    changed = true;
                }
                if ui.button("그만두기").clicked() {
                    self.confirm_forget = false;
                }
            } else if ui.button("학습한 단어 모두 잊기…").clicked() {
                self.confirm_forget = true;
            }
        });

        if changed {
            self.save_dictionary();
        }
    }

    fn about(&mut self, ui: &mut egui::Ui) {
        section(ui, "Araso for Windows", &format!("버전 {}", env!("CARGO_PKG_VERSION")));
        ui.label("영문 자판으로 치면 단어마다 영어인지 한국어인지 알아보고 맞는 글자로 넣습니다.");
        ui.label("macOS용 Araso(nyttastudio)를 Windows로 옮긴 비공식 포트입니다. 코드는 MIT, 단어 목록은 각 출처의 라이선스를 따릅니다.");

        section(ui, "링크", "");
        ui.horizontal(|ui| {
            ui.label("업스트림 (macOS Araso)");
            ui.hyperlink_to("nyttastudio.com/araso", UPSTREAM);
        });
        ui.horizontal(|ui| {
            ui.label("원본");
            ui.hyperlink_to("github.com/mununki/araso-windows", HOME);
        });
        ui.horizontal(|ui| {
            ui.label("문의·버그 신고");
            ui.hyperlink_to("github.com/mununki/araso-windows/issues", ISSUES);
        });

        section(ui, "파일", "설정과 학습한 단어는 이 폴더에 있습니다. 바꾸면 2초 안에 모든 앱에 반영됩니다.");
        if let Some(dir) = support_dir::url() {
            ui.horizontal(|ui| {
                ui.monospace(dir.display().to_string());
                if ui.button("폴더 열기").clicked() {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = std::process::Command::new("explorer.exe").arg(&dir).spawn();
                }
            });
        }
        if let Some(logging) = support_dir::file("logging") {
            let mut on = logging.exists();
            let label = "진단 기록 남기기 (키의 종류와 입력란의 종류만, 친 글자는 남기지 않음)";
            if ui.checkbox(&mut on, label).changed() {
                let result = if on { std::fs::write(&logging, "") } else { std::fs::remove_file(&logging) };
                self.report("진단 기록 설정", result);
            }
        }
    }
}

impl eframe::App for SettingsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.reread_dictionary();
        ui.ctx().request_repaint_after(Duration::from_secs(2));

        egui::Panel::top("tabs").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::General, "일반");
                ui.selectable_value(&mut self.tab, Tab::Apps, "앱별 규칙");
                ui.selectable_value(&mut self.tab, Tab::Words, "학습한 단어");
                ui.selectable_value(&mut self.tab, Tab::About, "정보");
            });
            ui.add_space(4.0);
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.add_space(4.0);
            match &self.status {
                Some(status) => ui.colored_label(ui.visuals().error_fg_color, status),
                None => ui.label(egui::RichText::new("바꾸면 바로 저장되고, 2초 안에 모든 앱에 반영됩니다.").weak()),
            };
            ui.add_space(4.0);
        });
        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::General => {
                scroll_area().show(ui, |ui| self.general(ui));
            }
            Tab::Apps => {
                scroll_area().show(ui, |ui| self.apps(ui));
            }
            Tab::Words => self.words(ui),
            Tab::About => self.about(ui),
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn title_bar_icon_comes_out_of_araso_ico() {
        let icon = super::icon().expect("the 48-pixel PNG in araso.ico");
        assert_eq!((icon.width, icon.height), (48, 48));
    }
}
