use wayland_client::QueueHandle;
use glyphon::FontSystem;
use cce_ui::engine::{Application, EngineState, LogicalPosition, LogicalSize, WindowSettings};
use cce_ui::widget::{
    MouseButton, ElementState, MouseScrollDelta, KeyEvent, Element,
    TextBox, Key, Dropdown
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
    menu_dropdown: cce_ui::widget::Adapted<Dropdown>,
    
    // Editor TextBox
    editor: cce_ui::widget::Adapted<TextBox>,
    
    // File state
    current_file_path: Option<std::path::PathBuf>,
    
    // UI state
    width: u32,
    height: u32,
    scale_factor: f64,
    // Shapes the editor's glyph advances (prepare_text) — load-bearing for cursor↔pixel
    // mapping; all rendered text is display-list prims shaped by the engine.
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

    /// The toolbar/status-bar chrome text — everything not owned by a widget (widget text
    /// comes from the paint walk). Emitted as display-list prims in the system monospace
    /// family, matching the app's legacy hand-shaped look.
    fn push_chrome_text(&self, pc: &mut cce_ui::scene::paint::PaintCtx) {
        let mono = || Some(cce_ui::layout::get_system_monospace_font().to_string());

        // 1. File path info in the toolbar
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
        pc.text_with(format!("File: {}", display_name), 420.0, 15.0, 12.0, [0xdd, 0xdd, 0xe2], mono(), None);

        // 2. Status Bar indicators
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
        pc.text_with(
            format!("Line: {}, Col: {} | Length: {} chars", logical_line, logical_col, text_src.chars().count()),
            15.0,
            self.height as f32 - 20.0,
            11.0,
            [0x83, 0x83, 0x8a],
            mono(),
            None,
        );

        if let Some((msg, is_error)) = &self.status_message {
            let color = if *is_error { [0xfa, 0x52, 0x52] } else { [0x40, 0xc0, 0x57] };
            pc.text_with(
                msg.clone(),
                (self.width as f32 - 400.0).max(300.0),
                self.height as f32 - 20.0,
                11.0,
                color,
                mono(),
                None,
            );
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
            font_system: cce_ui::create_font_system_with_system_fonts(),
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
                Element::focus(&mut self.editor);

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
                            Element::focus(&mut self.editor);
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

    fn display_list(&mut self, size: LogicalSize, scale: f64) -> Option<cce_ui::scene::paint::DisplayList> {
        // Phase 6 single paint path: the whole frame — chrome geometry, chrome text, and the
        // two top-level widgets (menu_dropdown, editor) walked into the list — is built here.
        // Widget text comes from the paint walk (Adapted::paint_self serves per-widget fonts).
        if !self.widgets_registered {
            self.widgets_registered = true;
            let self_ptr = self as *mut Self;
            unsafe {
                self.ui_context.register_widget(self.menu_dropdown.base().unwrap().id(), (*self_ptr).menu_dropdown.as_ptr_mut());
                self.ui_context.register_widget(self.editor.base().unwrap().id(), (*self_ptr).editor.as_ptr_mut());
            }
        }

        if self.initial_focus {
            self.initial_focus = false;
            self.ui_context.set_focused(&mut self.editor);
            Element::focus(&mut self.editor);
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

            // Glyph-advance shaping — load-bearing for cursor↔pixel mapping.
            self.editor.prepare_text(&mut self.font_system);
            self.needs_rebuild = false;

            self.ui_context.rebuild_spatial_grid();
        }

        // Popovers registration (since this app bypasses the layout engine)
        self.ui_context.clear_popovers();
        cce_ui::widget::popovers::clear();
        if self.menu_dropdown.popover_rect().is_some() {
            self.ui_context.register_popover(&self.menu_dropdown);
            cce_ui::widget::popovers::register(&self.menu_dropdown);
        }

        use cce_ui::scene::layout::Rect;
        let mut pc = cce_ui::scene::paint::PaintCtx::new();
        let w = self.width as f32;
        let h = self.height as f32;
        let status_y = h - 30.0;
        let radius = cce_ui::colors::backplate_corner_radius();
        if radius > 0.1 {
            pc.rounded_rect(Rect { x: 0.0, y: 0.0, width: w, height: h }, radius, (true, true, true, true), [0.05, 0.05, 0.07, 1.0]);
            pc.rounded_rect(Rect { x: 0.0, y: 0.0, width: w, height: 42.0 }, radius, (true, true, false, false), [0.08, 0.08, 0.12, 1.0]);
            pc.rounded_rect(Rect { x: 0.0, y: status_y, width: w, height: 30.0 }, radius, (false, false, true, true), [0.08, 0.08, 0.10, 1.0]);
        } else {
            pc.quad(Rect { x: 0.0, y: 0.0, width: w, height: h }, [0.05, 0.05, 0.07, 1.0]);
            pc.quad(Rect { x: 0.0, y: 0.0, width: w, height: 42.0 }, [0.08, 0.08, 0.12, 1.0]);
            pc.quad(Rect { x: 0.0, y: status_y, width: w, height: 30.0 }, [0.08, 0.08, 0.10, 1.0]);
        }
        pc.quad(Rect { x: 0.0, y: 42.0, width: w, height: 1.0 }, [0.18, 0.18, 0.22, 1.0]);
        pc.quad(Rect { x: 0.0, y: status_y, width: w, height: 1.0 }, [0.18, 0.18, 0.22, 1.0]);

        self.push_chrome_text(&mut pc);

        let menu: *mut (dyn cce_ui::widget::Element + 'static) = self.menu_dropdown.as_ptr_mut();
        let editor: *mut (dyn cce_ui::widget::Element + 'static) = self.editor.as_ptr_mut();
        cce_ui::scene::painter::paint_root_into(&self.ui_context, menu, &mut pc);
        cce_ui::scene::painter::paint_root_into(&self.ui_context, editor, &mut pc);
        Some(pc.finish())
    }

    fn display_list_text(&self) -> bool {
        true
    }

    fn render_popovers(&self, pc: &mut dyn cce_ui::layout::RenderTarget) {
        cce_ui::layout::render_popovers(pc, &self.ui_context);
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
