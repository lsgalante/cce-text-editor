use wayland_client::QueueHandle;
use glyphon::{FontSystem, Buffer, Metrics, Attrs};
use cce_ui::engine::{Application, EngineState, LogicalPosition, LogicalSize, WindowSettings};
use cce_ui::widget::{
    MouseButton, ElementState, MouseScrollDelta, KeyEvent, TextItem, Element,
    TextBox, TextLabel, Key, Dropdown
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
    // File menu dropdown
    menu_dropdown: Dropdown,
    
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
    ui_context: cce_ui::context::UiContext,
    ctrl_pressed: bool,
    initial_focus: bool,
    status_message: Option<(String, bool)>,
    widgets_registered: bool,
}

impl TextEditorApp {
    fn pick_file_to_open(&self) -> Option<std::path::PathBuf> {
        let output = std::process::Command::new("/home/lsgalante/.local/bin/cce-files")
            .arg("--select")
            .output()
            .or_else(|_| {
                std::process::Command::new("cce-files")
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
        let path_opt = std::process::Command::new("/home/lsgalante/.local/bin/cce-files")
            .arg("--save")
            .output()
            .or_else(|_| {
                std::process::Command::new("cce-files")
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
        let scale = cce_ui::scale::scale_factor();
        let mut labels = Vec::new();

        // 1. Menu dropdown labels
        labels.extend(self.menu_dropdown.text_labels());

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
                "monospace" => glyphon::Family::Name(cce_ui::layout::get_system_monospace_font()),
                "sans-serif" => glyphon::Family::Name(cce_ui::layout::get_system_monospace_font()),
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
            let family_val = glyphon::Family::Name(cce_ui::layout::get_system_monospace_font());
            let attrs = Attrs::new().family(family_val);
            buf.set_text(&mut self.font_system, &label.text, attrs, glyphon::Shaping::Advanced);
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

    fn ui_context(&self) -> Option<&cce_ui::context::UiContext> {
        Some(&self.ui_context)
    }

    fn ui_context_mut(&mut self) -> Option<&mut cce_ui::context::UiContext> {
        Some(&mut self.ui_context)
    }

    fn new(_qh: &QueueHandle<EngineState<Self>>, _sender: calloop::channel::Sender<Self::Message>) -> Self {
        let dropdown_options = vec![
            "New".to_string(),
            "Open...".to_string(),
            "Save".to_string(),
            "Save As...".to_string(),
            "-".to_string(),
            "Exit".to_string(),
        ];
        let mut menu_dropdown = Dropdown::new(dropdown_options, 0).with_custom_display_text("File");
        menu_dropdown.set_rect(10.0, 8.0, 70.0, 26.0);

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
            menu_dropdown,
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
            ui_context: cce_ui::context::UiContext::new(),
            ctrl_pressed: false,
            initial_focus: true,
            status_message: None,
            widgets_registered: false,
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
        if !self.widgets_registered {
            self.widgets_registered = true;
            let self_ptr = self as *mut Self;
            unsafe {
                self.ui_context.register_widget(self.menu_dropdown.base().unwrap().id(), &mut (*self_ptr).menu_dropdown as *mut Dropdown as *mut (dyn Element + 'static));
                self.ui_context.register_widget(self.editor.base().unwrap().id(), &mut (*self_ptr).editor as *mut TextBox as *mut (dyn Element + 'static));
            }
        }

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

            // Set menu dropdown dimensions and coordinates
            self.menu_dropdown.set_rect(10.0, 8.0, 70.0, 26.0);

            // TextBox occupies the remaining space between the top bar and status bar
            let editor_w = (self.width as f32 - 20.0).max(100.0);
            let editor_h = (self.height as f32 - 92.0).max(100.0);
            self.editor.set_rect(10.0, 52.0, editor_w, editor_h);

            self.rebuild_text_items();
            self.needs_rebuild = false;

            self.ui_context.rebuild_spatial_grid();
        }

        let radius = cce_ui::colors::backplate_corner_radius();
        if radius <= 0.1 {
            // 1. Editor Window Background (slate-dark design)
            quads.push((0.0, 0.0, self.width as f32, self.height as f32, [0.05, 0.05, 0.07, 1.0]));

            // 2. Toolbar Header quads
            quads.push((0.0, 0.0, self.width as f32, 42.0, [0.08, 0.08, 0.12, 1.0]));

            // 3. Status Bar quads
            let status_y = self.height as f32 - 30.0;
            quads.push((0.0, status_y, self.width as f32, 30.0, [0.08, 0.08, 0.10, 1.0]));
        }

        // Horizontal split lines (borders)
        quads.push((0.0, 42.0, self.width as f32, 1.0, [0.18, 0.18, 0.22, 1.0]));
        let status_y = self.height as f32 - 30.0;
        quads.push((0.0, status_y, self.width as f32, 1.0, [0.18, 0.18, 0.22, 1.0]));

        // 4. Menu dropdown graphics
        quads.extend(self.menu_dropdown.all_quads(&self.ui_context));

        // 5. TextBox Editor graphics
        quads.extend(self.editor.all_quads(&self.ui_context));

        // 6. Popovers registration (since this app bypasses the layout engine)
        self.ui_context.clear_popovers();
        cce_ui::widget::popovers::clear();
        if self.menu_dropdown.popover_rect().is_some() {
            self.ui_context.register_popover(&self.menu_dropdown);
            cce_ui::widget::popovers::register(&self.menu_dropdown);
        }
    }

    fn view_rounded_quads(&mut self, quads: &mut Vec<(f32, f32, f32, f32, f32, [f32; 4], (bool, bool, bool, bool))>, _size: LogicalSize, _scale: f64) {
        let radius = cce_ui::colors::backplate_corner_radius();
        if radius > 0.1 {
            // 1. Editor Window Background (rounded)
            quads.push((0.0, 0.0, self.width as f32, self.height as f32, radius, [0.05, 0.05, 0.07, 1.0], (true, true, true, true)));

            // 2. Toolbar Header (rounded at top)
            quads.push((0.0, 0.0, self.width as f32, 42.0, radius, [0.08, 0.08, 0.12, 1.0], (true, true, false, false)));

            // 3. Status Bar (rounded at bottom)
            let status_y = self.height as f32 - 30.0;
            quads.push((0.0, status_y, self.width as f32, 30.0, radius, [0.08, 0.08, 0.10, 1.0], (false, false, true, true)));
        }

        quads.extend(self.menu_dropdown.all_rounded_quads(&self.ui_context));
        quads.extend(self.editor.all_rounded_quads(&self.ui_context));
    }

    fn render_popovers(&self, pc: &mut dyn cce_ui::layout::RenderTarget) {
        cce_ui::layout::render_popovers(pc, &self.ui_context);
    }

    fn text_items(&self) -> &[TextItem] {
        &self.text_items
    }

    fn handle_pointer_move(&mut self, pos: LogicalPosition, needs_rebuild: &mut bool) {
        let mut changed = false;
        let px = pos.x as f32;
        let py = pos.y as f32;

        if self.menu_dropdown.on_cursor_moved(px, py, &mut self.ui_context) { changed = true; }
        
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

        if self.menu_dropdown.mouse_input(button, state, px, py, &mut self.ui_context) {
            changed = true;
            if self.menu_dropdown.take_change() {
                let selected_idx = self.menu_dropdown.selected;
                if selected_idx < self.menu_dropdown.options.len() {
                    let option_text = &self.menu_dropdown.options[selected_idx];
                    match option_text.as_str() {
                        "New" => {
                            msg_out = Some(AppMessage::NewDocument);
                        }
                        "Open..." => {
                            msg_out = Some(AppMessage::OpenDocument);
                        }
                        "Save" => {
                            msg_out = Some(AppMessage::SaveDocument);
                        }
                        "Save As..." => {
                            msg_out = Some(AppMessage::SaveDocumentAs);
                        }
                        "Exit" => {
                            msg_out = Some(AppMessage::Exit);
                        }
                        _ => {}
                    }
                }
            }
        } else if self.editor.mouse_input(button, state, px, py, &mut self.ui_context) {
            changed = true;
        } else if state == ElementState::Pressed && button == MouseButton::Left {
            self.editor.unfocus();
            changed = true;
        }

        if changed || msg_out.is_some() {
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
            if self.menu_dropdown.keyboard_input(event, &mut self.ui_context) {
                handled = true;
                if self.menu_dropdown.take_change() {
                    let selected_idx = self.menu_dropdown.selected;
                    if selected_idx < self.menu_dropdown.options.len() {
                        let option_text = &self.menu_dropdown.options[selected_idx];
                        match option_text.as_str() {
                            "New" => {
                                msg_out = Some(AppMessage::NewDocument);
                            }
                            "Open..." => {
                                msg_out = Some(AppMessage::OpenDocument);
                            }
                            "Save" => {
                                msg_out = Some(AppMessage::SaveDocument);
                            }
                            "Save As..." => {
                                msg_out = Some(AppMessage::SaveDocumentAs);
                            }
                            "Exit" => {
                                msg_out = Some(AppMessage::Exit);
                            }
                            _ => {}
                        }
                    }
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
    
    cce_ui::engine::run::<TextEditorApp>();
}
