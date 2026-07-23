use std::time::Instant;

use chat::{ChatEvent, ChatService, Role};

#[derive(Debug, PartialEq)]
pub enum InputMode {
    Normal,
    Insert,
}

pub enum UserEvent {
    Chat(ChatEvent),
    Input(InputEvent),
}

pub enum InputEvent {
    Key(crossterm::event::KeyEvent),
    ScrollUp,
    ScrollDown,
}

pub struct Message {
    pub role: Role,
    pub content: String,
}

pub enum CachedLine {
    Role(Role),
    Content(String),
    Spacer,
}

pub struct RenderCache {
    pub width: u16,
    pub generation: u64,
    pub lines: Vec<CachedLine>,
    pub last_assistant_line: Option<usize>,
}

impl RenderCache {
    pub fn new() -> Self {
        Self {
            width: 0,
            generation: 0,
            lines: Vec::new(),
            last_assistant_line: None,
        }
    }

    pub fn rebuild(&mut self, messages: &[Message], width: u16, generation: u64) {
        self.lines.clear();
        self.last_assistant_line = None;
        let opts = textwrap::Options::new(width as usize)
            .break_words(false)
            .wrap_algorithm(textwrap::WrapAlgorithm::FirstFit);

        for (i, msg) in messages.iter().enumerate() {
            let role_idx = self.lines.len();
            self.lines.push(CachedLine::Role(msg.role));
            if i == messages.len() - 1 && msg.role == Role::Assistant {
                self.last_assistant_line = Some(role_idx);
            }
            if msg.content.is_empty() {
                self.lines.push(CachedLine::Content(String::new()));
            } else {
                for line in textwrap::wrap(&msg.content, &opts) {
                    self.lines.push(CachedLine::Content(line.into_owned()));
                }
            }
            self.lines.push(CachedLine::Spacer);
        }
        self.width = width;
        self.generation = generation;
    }
}

pub struct App {
    pub svc: ChatService,
    pub sessions: Vec<(String, String)>,
    pub cur_session: Option<String>,
    pub messages: Vec<Message>,
    pub input: String,
    pub cursor: usize,
    pub input_mode: InputMode,
    pub scroll: usize,
    pub show_help: bool,
    pub status: String,
    pub waiting: bool,
    pub thinking_tick: usize,
    pub thinking_start: Option<Instant>,
    pub command_suggestions: Vec<&'static str>,
    pub show_suggestions: bool,
    pub should_quit: bool,
    pub dirty: bool,
    pub draw_count: u64,
    pub render_cache: RenderCache,
    pub layout_generation: u64,
}

impl App {
    pub fn new(svc: ChatService) -> Self {
        Self {
            svc,
            sessions: Vec::new(),
            cur_session: None,
            messages: Vec::new(),
            input: String::new(),
            cursor: 0,
            input_mode: InputMode::Insert,
            scroll: 0,
            show_help: false,
            status: String::new(),
            waiting: false,
            thinking_tick: 0,
            thinking_start: None,
            command_suggestions: vec![
                "/new",
                "/switch <id|title>",
                "/rename <title>",
                "/delete <id>",
                "/list",
                "/help",
                "/quit",
            ],
            show_suggestions: false,
            should_quit: false,
            dirty: true,
            draw_count: 0,
            render_cache: RenderCache::new(),
            layout_generation: 0,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub async fn refresh_sessions(&mut self) {
        if let Ok(sessions) = self.svc.list_sessions().await {
            self.sessions = sessions;
        }
        self.cur_session = self.svc.current_session().await.map(|id| id.to_string());
    }

    pub fn add_message(&mut self, role: Role, content: impl Into<String>) {
        self.messages.push(Message {
            role,
            content: content.into(),
        });
        self.layout_generation += 1;
    }

    pub fn clear_messages(&mut self) {
        self.messages.clear();
        self.layout_generation += 1;
    }

    pub fn update_last_message_content(&mut self, content: &str) {
        if let Some(last) = self.messages.last_mut() {
            last.content.push_str(content);
            self.layout_generation += 1;
        }
    }
}
