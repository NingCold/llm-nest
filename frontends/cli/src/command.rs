#[derive(Debug)]
pub enum Command {
    New,
    Switch { target: String },
    Rename { title: String },
    Delete { id: String },
    List,
    Models,
    Model { target: String },
    Effort { level: Option<String> },
    Current,
    Reload,
    Refresh { provider: String },
    Help,
    Quit,
}

impl Command {
    pub fn parse(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        if !trimmed.starts_with('/') {
            return None;
        }
        let parts: Vec<&str> = trimmed[1..].splitn(2, char::is_whitespace).collect();
        match parts[0] {
            "new" => Some(Command::New),
            "switch" => Some(Command::Switch {
                target: parts.get(1).unwrap_or(&"").to_string(),
            }),
            "rename" => Some(Command::Rename {
                title: parts.get(1).unwrap_or(&"").to_string(),
            }),
            "delete" => Some(Command::Delete {
                id: parts.get(1).unwrap_or(&"").to_string(),
            }),
            "list" => Some(Command::List),
            "models" => Some(Command::Models),
            "model" => Some(Command::Model {
                target: parts.get(1).unwrap_or(&"").trim().to_string(),
            }),
            "effort" => Some(Command::Effort {
                level: parts
                    .get(1)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
            }),
            "current" => Some(Command::Current),
            "reload" => Some(Command::Reload),
            "refresh" => Some(Command::Refresh {
                provider: parts.get(1).unwrap_or(&"").trim().to_string(),
            }),
            "help" => Some(Command::Help),
            "quit" => Some(Command::Quit),
            "exit" => Some(Command::Quit),
            _ => None,
        }
    }

    pub fn help_text() -> &'static str {
        "\
LLM Nest (llmn) 命令
====================

  /new               创建新会话并切换到该会话
  /switch <id|标题>  切换到指定会话
  /rename <标题>     重命名当前会话
  /delete <id>       删除指定会话
  /list              列出所有会话
  /models            列出所有可用模型
  /model <provider/model>
                     切换到指定模型（也支持裸模型名，跨 provider 唯一匹配时可用）
  /effort <off|low|medium|high|max>
                     设置当前模型的 reasoning effort（无参时显示当前值）
  /current           显示当前模型的 provider / model / effort
  /reload            重新加载 config/llmn.toml（文件改动也会自动热更新）
  /refresh <provider> 从该 provider 的 GET /models 拉取新模型并写回 config/llmn.toml
  /help              显示此帮助
  /quit              退出程序

提示：
  - 首次使用会自动创建默认会话
  - /switch 支持 UUID 和会话标题两种方式
  - 直接输入文本即可开始对话"
    }
}
