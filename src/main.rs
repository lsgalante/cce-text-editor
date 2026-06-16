use wayland_client::QueueHandle;
use glyphon::{FontSystem, Buffer, Metrics, Attrs};
use clear_ui::engine::{Application, EngineState, LogicalPosition, LogicalSize, WindowSettings};
use clear_ui::widget::{
    MouseButton, ElementState, MouseScrollDelta, KeyEvent, TextItem, Element,
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
    ui_context: clear_ui::context::UiContext,
    ctrl_pressed: bool,
    initial_focus: bool,
    status_message: Option<(String, bool)>,
}

impl TextEditorApp {
    fn pick_file_to_open(&self) -> Option<std::path::PathBuf> {
        let output = std::process::Command::new("/home/lsgalante/.local/bin/cce-filesystem-interface")
            .arg("--select")
            .output()
            .or_else(|_| {
                std::process::Command::new("cce-filesystem-interface")
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
        let path_opt = std::process::Command::new("/home/lsgalante/.local/bin/cce-filesystem-interface")
            .arg("--save")
            .output()
            .or_else(|_| {
                std::process::Command::new("cce-filesystem-interface")
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
            match std::fs::write(&path, content) {
                Ok(_) => {
                    self.current_file_path = Some(path.clone());
                    if self.editor.editing {
                        self.editor.text = self.editor.edit_buffer.clone();
                    }
                    self.status_message = Some((format!("Saved successfully to {}", path.file_name().unwrap_or_default().to_string_lossy()), false));
                    *needs_rebuild = true;
                    self.needs_rebuild = true;
                }
                Err(e) => {
                    self.status_message = Some((format!("Error saving file: {}", e), true));
                    *needs_rebuild = true;
                    self.needs_rebuild = true;
                }
            }
        }
    }

    fn rebuild_text_items(&mut self) {
        self.editor.prepare_text(&mut self.font_system);
        self.text_items.clear();
        let scale = clear_ui::scale::scale_factor();
        let mut labels = Vec::new();

        // 1. Button labels
        labels.extend(self.btn_new.text_labels());
        labels.extend(self.btn_open.text_labels());
        labels.extend(self.btn_save.text_labels());
        labels.extend(self.btn_save_as.text_labels());
        labels.extend(self.btn_exit.text_labels());

        // 2. File path info in the toolbar
        let is_dirty = if self.editor.editing {
            self.editor.text != self.editor.edit_buffer
        } else {
            false
        };
        let file_name_str = match &self.current_file_path {
            Some(path) => path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            None => "Untitled".to_string(),
        };
        let display_name = if is_dirty {
            format!("*{}", file_name_str)
        } else {
            file_name_str
        };
        labels.push(TextLabel {
            text: format!("File: {}", display_name),
            x: 420.0,
            y: 15.0,
            font_size: 12.0,
            color: [0xdd, 0xdd, 0xe2],
        });

        // 3. Editor text labels
        let font_family = self.editor.font_family.clone();
        for (label, bounds) in self.editor.text_labels_with_bounds(&self.ui_context) {
            let physical_size = label.font_size * scale;
            let metrics = Metrics::new(physical_size, physical_size * 1.4);
            let mut buf = Buffer::new(&mut self.font_system, metrics);
            let family_val = match font_family.as_str() {
                "monospace" => glyphon::Family::Name(clear_ui::layout::get_system_monospace_font()),
                "sans-serif" => glyphon::Family::SansSerif,
                "serif" => glyphon::Family::Serif,
                _ => glyphon::Family::Name(&font_family),
            };
            let attrs = Attrs::new().family(family_val);
            buf.set_text(&mut self.font_system, &label.text, attrs, glyphon::Shaping::Advanced);
            buf.shape_until_scroll(&mut self.font_system, true);
            self.text_items.push(TextItem {
                buffer: buf,
                x: label.x,
                y: label.y,
                color: glyphon::Color::rgb(label.color[0], label.color[1], label.color[2]),
                bounds,
            });
        }

        // 4. Status Bar indicators
        let text_src = if self.editor.editing { &self.editor.edit_buffer } else { &self.editor.text };
        let mut logical_line = 1;
        let mut logical_col = 1;
        for (idx, ch) in text_src.chars().enumerate() {
            if idx >= self.editor.cursor_idx {
                break;
            }
            if ch == '\n' {
                logical_line += 1;
                logical_col = 1;
            } else {
                logical_col += 1;
            }
        }

        labels.push(TextLabel {
            text: format!("Line: {}, Col: {} | Length: {} chars", logical_line, logical_col, text_src.chars().count()),
            x: 15.0,
            y: self.height as f32 - 20.0,
            font_size: 11.0,
            color: [0x83, 0x83, 0x8a],
        });

        if let Some((msg, is_error)) = &self.status_message {
            let color = if *is_error { [0xfa, 0x52, 0x52] } else { [0x40, 0xc0, 0x57] };
            labels.push(TextLabel {
                text: msg.clone(),
                x: (self.width as f32 - 400.0).max(300.0),
                y: self.height as f32 - 20.0,
                font_size: 11.0,
                color,
            });
        }

        // 5. Build static text items
        for label in labels {
            let physical_size = label.font_size * scale;
            let metrics = Metrics::new(physical_size, physical_size * 1.4);
            let mut buf = Buffer::new(&mut self.font_system, metrics);
            buf.set_text(&mut self.font_system, &label.text, Attrs::new(), glyphon::Shaping::Advanced);
            buf.shape_until_scroll(&mut self.font_system, true);
            self.text_items.push(TextItem {
                buffer: buf,
                x: label.x,
                y: label.y,
                color: glyphon::Color::rgb(label.color[0], label.color[1], label.color[2]),
                bounds: None,
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
            font_system: {
                let mut fs = FontSystem::new();
                fs.db_mut().load_fonts_dir("/home/lsgalante/Dropbox/Fonts");
                fs
            },
            needs_rebuild: true,
            ui_context: clear_ui::context::UiContext::new(),
            ctrl_pressed: false,
            initial_focus: true,
            status_message: None,
        }
    }

    fn settings(&self) -> WindowSettings {
        WindowSettings {
            title: "Clear Text Editor".to_string(),
            app_id: "cce-text-editor".to_string(),
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
                self.status_message = None;

                self.ui_context.set_focused(&mut self.editor);
                TextBox::focus(&mut self.editor);

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
                            self.current_file_path = Some(path.clone());
                            self.status_message = Some((format!("Opened {}", path.file_name().unwrap_or_default().to_string_lossy()), false));

                            self.ui_context.set_focused(&mut self.editor);
                            TextBox::focus(&mut self.editor);
                        }
                        Err(e) => {
                            self.status_message = Some((format!("Error opening file: {}", e), true));
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
                    match std::fs::write(&path, content) {
                        Ok(_) => {
                            if self.editor.editing {
                                self.editor.text = self.editor.edit_buffer.clone();
                            }
                            self.status_message = Some((format!("Saved successfully to {}", path.file_name().unwrap_or_default().to_string_lossy()), false));
                        }
                        Err(e) => {
                            self.status_message = Some((format!("Error saving file: {}", e), true));
                        }
                    }
                    *needs_rebuild = true;
                    self.needs_rebuild = true;
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
        if self.initial_focus {
            self.initial_focus = false;
            self.ui_context.set_focused(&mut self.editor);
            TextBox::focus(&mut self.editor);
            self.needs_rebuild = true;
        }
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

        if self.btn_new.on_cursor_moved(px, py, &mut self.ui_context) { changed = true; }
        if self.btn_open.on_cursor_moved(px, py, &mut self.ui_context) { changed = true; }
        if self.btn_save.on_cursor_moved(px, py, &mut self.ui_context) { changed = true; }
        if self.btn_save_as.on_cursor_moved(px, py, &mut self.ui_context) { changed = true; }
        if self.btn_exit.on_cursor_moved(px, py, &mut self.ui_context) { changed = true; }
        
        if self.editor.on_cursor_moved(px, py, &mut self.ui_context) { changed = true; }

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

        if self.btn_new.mouse_input(button, state, px, py, &mut self.ui_context) {
            changed = true;
            if state == ElementState::Released && self.btn_new.take_click() {
                msg_out = Some(AppMessage::NewDocument);
            }
        }
        if self.btn_open.mouse_input(button, state, px, py, &mut self.ui_context) {
            changed = true;
            if state == ElementState::Released && self.btn_open.take_click() {
                msg_out = Some(AppMessage::OpenDocument);
            }
        }
        if self.btn_save.mouse_input(button, state, px, py, &mut self.ui_context) {
            changed = true;
            if state == ElementState::Released && self.btn_save.take_click() {
                msg_out = Some(AppMessage::SaveDocument);
            }
        }
        if self.btn_save_as.mouse_input(button, state, px, py, &mut self.ui_context) {
            changed = true;
            if state == ElementState::Released && self.btn_save_as.take_click() {
                msg_out = Some(AppMessage::SaveDocumentAs);
            }
        }
        if self.btn_exit.mouse_input(button, state, px, py, &mut self.ui_context) {
            changed = true;
            if state == ElementState::Released && self.btn_exit.take_click() {
                msg_out = Some(AppMessage::Exit);
            }
        }

        if self.editor.mouse_input(button, state, px, py, &mut self.ui_context) {
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

    fn handle_mouse_wheel(&mut self, delta: &MouseScrollDelta, _pos: LogicalPosition, needs_rebuild: &mut bool) {
        if self.ctrl_pressed {
            match delta {
                MouseScrollDelta::LineDelta(_, y) => {
                    if *y > 0.0 {
                        self.editor.font_size = (self.editor.font_size + 1.0).min(72.0);
                    } else if *y < 0.0 {
                        self.editor.font_size = (self.editor.font_size - 1.0).max(6.0);
                    }
                    *needs_rebuild = true;
                    self.needs_rebuild = true;
                }
                MouseScrollDelta::PixelDelta(pos) => {
                    if pos.y > 0.0 {
                        self.editor.font_size = (self.editor.font_size + 1.0).min(72.0);
                    } else if pos.y < 0.0 {
                        self.editor.font_size = (self.editor.font_size - 1.0).max(6.0);
                    }
                    *needs_rebuild = true;
                    self.needs_rebuild = true;
                }
            }
        }
    }

    fn handle_key_input(&mut self, event: &KeyEvent, needs_rebuild: &mut bool) -> Option<Self::Message> {
        self.ctrl_pressed = event.ctrl;

        // Clear status message when typing/key press occurs
        if event.state == ElementState::Pressed && self.status_message.is_some() {
            self.status_message = None;
            *needs_rebuild = true;
            self.needs_rebuild = true;
        }

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
                    "=" | "+" => {
                        self.editor.font_size = (self.editor.font_size + 1.0).min(72.0);
                        *needs_rebuild = true;
                        self.needs_rebuild = true;
                        handled = true;
                    }
                    "-" | "_" => {
                        self.editor.font_size = (self.editor.font_size - 1.0).max(6.0);
                        *needs_rebuild = true;
                        self.needs_rebuild = true;
                        handled = true;
                    }
                    _ => {}
                }
            }
        }

        if !handled {
            if self.editor.keyboard_input(event, &mut self.ui_context) {
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
