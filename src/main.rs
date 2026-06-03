use wayland_client::QueueHandle;
use glyphon::{FontSystem, Buffer, Metrics, Attrs};
use clear_ui::engine::{Application, EngineState, LogicalPosition, LogicalSize, WindowSettings};
use clear_ui::widget::{
    MouseButton, ElementState, MouseScrollDelta, KeyEvent, TextItem, Widget,
    TextBox, Button, TextLabel, Key
};

#[derive(Debug, Clone)]
enum AppMessage {
    Exit,
    NewDocument,
    OpenDocument,
    SaveDocument,
    SaveDocumentAs,
}

struct TextEditorApp {
    // Buttons
    btn_new: Button,
    btn_open: Button,
    btn_save: Button,
    btn_save_as: Button,
    btn_exit: Button,
    
    // Editor TextBox
    editor: TextBox,
    
    // File state
    current_file_path: Option<std::path::PathBuf>,
    
    // UI state
    width: u32,
    height: u32,
    scale_factor: f64,
    text_items: Vec<TextItem>,
    font_system: FontSystem,
    needs_rebuild: bool,
}

impl TextEditorApp {
    fn pick_file_to_open(&self) -> Option<std::path::PathBuf> {
        let output = std::process::Command::new("/home/lsgalante/.local/bin/clear-filesystem-interface")
            .arg("--select")
            .output()
            .or_else(|_| {
                std::process::Command::new("clear-filesystem-interface")
                    .arg("--select")
                    .output()
            })
            .ok()?;
        
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let trimmed = stdout.trim();
            if !trimmed.is_empty() {
                return Some(std::path::PathBuf::from(trimmed));
            }
        }
        None
    }

    fn perform_save_as(&mut self, needs_rebuild: &mut bool) {
        let path_opt = std::process::Command::new("/home/lsgalante/.local/bin/clear-filesystem-interface")
            .arg("--save")
            .output()
            .or_else(|_| {
                std::process::Command::new("clear-filesystem-interface")
                    .arg("--save")
                    .output()
            })
            .ok()
            .and_then(|output| {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let trimmed = stdout.trim();
                    if !trimmed.is_empty() {
                        Some(std::path::PathBuf::from(trimmed))
                    } else {
                        None
                    }
                } else {
                    None
                }
            });

        if let Some(path) = path_opt {
            let content = if self.editor.editing { &self.editor.edit_buffer } else { &self.editor.text };
            if let Err(e) = std::fs::write(&path, content) {
                eprintln!("Error saving file: {}", e);
            } else {
                self.current_file_path = Some(path);
                *needs_rebuild = true;
                self.needs_rebuild = true;
            }
        }
    }

    fn rebuild_text_items(&mut self) {
        self.text_items.clear();
        let mut labels = Vec::new();

        // 1. Button labels
        labels.extend(self.btn_new.text_labels());
        labels.extend(self.btn_open.text_labels());
        labels.extend(self.btn_save.text_labels());
        labels.extend(self.btn_save_as.text_labels());
        labels.extend(self.btn_exit.text_labels());

        // 2. File path info in the toolbar
        let file_name_str = match &self.current_file_path {
            Some(path) => path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            None => "Untitled".to_string(),
        };
        labels.push(TextLabel {
            text: format!("File: {}", file_name_str),
            x: 420.0,
            y: 15.0,
            font_size: 12.0,
            color: [0xdd, 0xdd, 0xe2],
        });

        // 3. Editor text labels
        let font_family = self.editor.font_family.clone();
        for label in self.editor.text_labels() {
            let metrics = Metrics::new(label.font_size, label.font_size * 1.4);
            let mut buf = Buffer::new(&mut self.font_system, metrics);
            let attrs = Attrs::new().family(glyphon::Family::Name(&font_family));
            buf.set_text(&mut self.font_system, &label.text, attrs, glyphon::Shaping::Advanced);
            buf.shape_until_scroll(&mut self.font_system, true);
            self.text_items.push(TextItem {
                buffer: buf,
                x: label.x,
                y: label.y,
                color: glyphon::Color::rgb(label.color[0], label.color[1], label.color[2]),
            });
        }

        // 4. Status Bar indicators
        let text_src = if self.editor.editing { &self.editor.edit_buffer } else { &self.editor.text };
        let char_width = self.editor.font_size * 0.6;
        let max_chars = (((self.editor.rect().2 - 16.0) / char_width).floor() as usize).max(1);
        let (_, index_map) = self.editor.wrap_text(max_chars);
        let cursor_idx = self.editor.cursor_idx.min(index_map.len() - 1);
        let (line, col) = index_map[cursor_idx];

        labels.push(TextLabel {
            text: format!("Line: {}, Col: {} | Length: {} chars", line + 1, col + 1, text_src.chars().count()),
            x: 15.0,
            y: self.height as f32 - 20.0,
            font_size: 11.0,
            color: [0x83, 0x83, 0x8a],
        });

        // 5. Build static text items
        for label in labels {
            let metrics = Metrics::new(label.font_size, label.font_size * 1.4);
            let mut buf = Buffer::new(&mut self.font_system, metrics);
            buf.set_text(&mut self.font_system, &label.text, Attrs::new(), glyphon::Shaping::Advanced);
            buf.shape_until_scroll(&mut self.font_system, true);
            self.text_items.push(TextItem {
                buffer: buf,
                x: label.x,
                y: label.y,
                color: glyphon::Color::rgb(label.color[0], label.color[1], label.color[2]),
            });
        }
    }
}

impl Application for TextEditorApp {
    type Message = AppMessage;

    fn new(_qh: &QueueHandle<EngineState<Self>>, _sender: calloop::channel::Sender<Self::Message>) -> Self {
        let btn_new = Button::new(10.0, 8.0, 70.0, 26.0).with_label("New");
        let btn_open = Button::new(90.0, 8.0, 70.0, 26.0).with_label("Open");
        let btn_save = Button::new(170.0, 8.0, 70.0, 26.0).with_label("Save");
        let btn_save_as = Button::new(250.0, 8.0, 75.0, 26.0).with_label("Save As");
        let btn_exit = Button::new(335.0, 8.0, 70.0, 26.0).with_label("Exit");

        // Monospace textbox setup
        let mut editor = TextBox::new(String::new())
            .with_multiline(true)
            .with_draw_bg_border(true)
            .with_max_width(None);
        editor.font_family = "monospace".to_string();
        editor.font_size = 13.0;

        // Auto-open path if passed as argv[1]
        let args: Vec<String> = std::env::args().collect();
        let mut current_file_path = None;
        if args.len() > 1 {
            let path = std::path::PathBuf::from(&args[1]);
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    editor.text = content;
                    editor.edit_buffer = editor.text.clone();
                    current_file_path = Some(path);
                }
            }
        }

        Self {
            btn_new,
            btn_open,
            btn_save,
            btn_save_as,
            btn_exit,
            editor,
            current_file_path,
            width: 800,
            height: 600,
            scale_factor: 1.0,
            text_items: Vec::new(),
            font_system: FontSystem::new(),
            needs_rebuild: true,
        }
    }

    fn settings(&self) -> WindowSettings {
        WindowSettings {
            title: "Clear Text Editor".to_string(),
            app_id: "clear-text-editor".to_string(),
            width: 800,
            height: 600,
            fullscreen: false,
            min_size: Some((500, 400)),
        }
    }

    fn update(&mut self, msg: Self::Message, needs_rebuild: &mut bool, exit: &mut bool) {
        match msg {
            AppMessage::Exit => {
                *exit = true;
            }
            AppMessage::NewDocument => {
                self.editor.text = String::new();
                self.editor.edit_buffer = String::new();
                self.editor.editing = false;
                self.editor.cursor_idx = 0;
                self.editor.select_anchor = None;
                self.current_file_path = None;
                *needs_rebuild = true;
                self.needs_rebuild = true;
            }
            AppMessage::OpenDocument => {
                if let Some(path) = self.pick_file_to_open() {
                    match std::fs::read_to_string(&path) {
                        Ok(content) => {
                            self.editor.text = content;
                            self.editor.edit_buffer = self.editor.text.clone();
                            self.editor.cursor_idx = 0;
                            self.editor.select_anchor = None;
                            self.editor.editing = false;
                            self.current_file_path = Some(path);
                        }
                        Err(e) => {
                            eprintln!("Error opening file: {}", e);
                        }
                    }
                    *needs_rebuild = true;
                    self.needs_rebuild = true;
                }
            }
            AppMessage::SaveDocument => {
                if self.current_file_path.is_some() {
                    let path = self.current_file_path.clone().unwrap();
                    let content = if self.editor.editing { &self.editor.edit_buffer } else { &self.editor.text };
                    if let Err(e) = std::fs::write(&path, content) {
                        eprintln!("Error saving file: {}", e);
                    }
                } else {
                    self.perform_save_as(needs_rebuild);
                }
            }
            AppMessage::SaveDocumentAs => {
                self.perform_save_as(needs_rebuild);
            }
        }
    }

    fn tick(&mut self, _dt: f32, _needs_rebuild: &mut bool) {}

    fn view(&mut self, quads: &mut Vec<(f32, f32, f32, f32, [f32; 4])>, size: LogicalSize, scale: f64) {
        let size_changed = self.width != size.width as u32 || self.height != size.height as u32 || self.scale_factor != scale;
        if self.needs_rebuild || size_changed {
            self.width = size.width as u32;
            self.height = size.height as u32;
            self.scale_factor = scale;

            // Set button dimensions and coordinates
            self.btn_new.set_rect(10.0, 8.0, 70.0, 26.0);
            self.btn_open.set_rect(90.0, 8.0, 70.0, 26.0);
            self.btn_save.set_rect(170.0, 8.0, 70.0, 26.0);
            self.btn_save_as.set_rect(250.0, 8.0, 75.0, 26.0);
            self.btn_exit.set_rect(335.0, 8.0, 70.0, 26.0);

            // TextBox occupies the remaining space between the top bar and status bar
            let editor_w = (self.width as f32 - 20.0).max(100.0);
            let editor_h = (self.height as f32 - 92.0).max(100.0);
            self.editor.set_rect(10.0, 52.0, editor_w, editor_h);

            self.rebuild_text_items();
            self.needs_rebuild = false;
        }

        // 1. Editor Window Background (slate-dark design)
        quads.push((0.0, 0.0, self.width as f32, self.height as f32, [0.05, 0.05, 0.07, 1.0]));

        // 2. Toolbar Header quads
        quads.push((0.0, 0.0, self.width as f32, 42.0, [0.08, 0.08, 0.12, 1.0]));
        quads.push((0.0, 42.0, self.width as f32, 1.0, [0.18, 0.18, 0.22, 1.0]));

        // 3. Status Bar quads
        let status_y = self.height as f32 - 30.0;
        quads.push((0.0, status_y, self.width as f32, 30.0, [0.08, 0.08, 0.10, 1.0]));
        quads.push((0.0, status_y, self.width as f32, 1.0, [0.18, 0.18, 0.22, 1.0]));

        // 4. Buttons graphics
        quads.extend(self.btn_new.extra_quads());
        quads.extend(self.btn_open.extra_quads());
        quads.extend(self.btn_save.extra_quads());
        quads.extend(self.btn_save_as.extra_quads());
        quads.extend(self.btn_exit.extra_quads());

        // 5. TextBox Editor graphics
        quads.extend(self.editor.extra_quads());
    }

    fn text_items(&self) -> &[TextItem] {
        &self.text_items
    }

    fn handle_pointer_move(&mut self, pos: LogicalPosition, needs_rebuild: &mut bool) {
        let mut changed = false;
        let px = pos.x as f32;
        let py = pos.y as f32;

        if self.btn_new.on_cursor_moved(px, py) { changed = true; }
        if self.btn_open.on_cursor_moved(px, py) { changed = true; }
        if self.btn_save.on_cursor_moved(px, py) { changed = true; }
        if self.btn_save_as.on_cursor_moved(px, py) { changed = true; }
        if self.btn_exit.on_cursor_moved(px, py) { changed = true; }
        
        if self.editor.on_cursor_moved(px, py) { changed = true; }

        if changed {
            *needs_rebuild = true;
            self.needs_rebuild = true;
        }
    }

    fn handle_mouse_input(&mut self, button: MouseButton, state: ElementState, pos: LogicalPosition, needs_rebuild: &mut bool) -> Option<Self::Message> {
        let mut changed = false;
        let mut msg_out = None;
        let px = pos.x as f32;
        let py = pos.y as f32;

        if self.btn_new.mouse_input(button, state, px, py) {
            changed = true;
            if state == ElementState::Released && self.btn_new.take_click() {
                msg_out = Some(AppMessage::NewDocument);
            }
        }
        if self.btn_open.mouse_input(button, state, px, py) {
            changed = true;
            if state == ElementState::Released && self.btn_open.take_click() {
                msg_out = Some(AppMessage::OpenDocument);
            }
        }
        if self.btn_save.mouse_input(button, state, px, py) {
            changed = true;
            if state == ElementState::Released && self.btn_save.take_click() {
                msg_out = Some(AppMessage::SaveDocument);
            }
        }
        if self.btn_save_as.mouse_input(button, state, px, py) {
            changed = true;
            if state == ElementState::Released && self.btn_save_as.take_click() {
                msg_out = Some(AppMessage::SaveDocumentAs);
            }
        }
        if self.btn_exit.mouse_input(button, state, px, py) {
            changed = true;
            if state == ElementState::Released && self.btn_exit.take_click() {
                msg_out = Some(AppMessage::Exit);
            }
        }

        if self.editor.mouse_input(button, state, px, py) {
            changed = true;
        } else if state == ElementState::Pressed && button == MouseButton::Left {
            self.editor.unfocus();
            changed = true;
        }

        if changed {
            *needs_rebuild = true;
            self.needs_rebuild = true;
        }

        msg_out
    }

    fn handle_mouse_wheel(&mut self, _delta: &MouseScrollDelta, _pos: LogicalPosition, _needs_rebuild: &mut bool) {}

    fn handle_key_input(&mut self, event: &KeyEvent, needs_rebuild: &mut bool) -> Option<Self::Message> {
        let mut handled = false;
        let mut msg_out = None;

        // Custom keyboard shortcuts
        if event.ctrl && event.state == ElementState::Pressed {
            if let Key::Character(ref ch) = event.logical_key {
                match ch.to_lowercase().as_str() {
                    "n" => {
                        msg_out = Some(AppMessage::NewDocument);
                        handled = true;
                    }
                    "o" => {
                        msg_out = Some(AppMessage::OpenDocument);
                        handled = true;
                    }
                    "s" => {
                        msg_out = Some(AppMessage::SaveDocument);
                        handled = true;
                    }
                    "q" => {
                        msg_out = Some(AppMessage::Exit);
                        handled = true;
                    }
                    _ => {}
                }
            }
        }

        if !handled {
            if self.editor.keyboard_input(event) {
                handled = true;
            }
        }

        if handled {
            *needs_rebuild = true;
            self.needs_rebuild = true;
        }

        msg_out
    }
}

fn main() {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let _guard = rt.enter();
    
    clear_ui::engine::run::<TextEditorApp>();
}
