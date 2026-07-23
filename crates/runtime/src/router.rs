use dashmap::DashMap;

pub struct Router {
    routes: DashMap<String, String>,
}

impl Router {
    pub fn new() -> Self {
        Self { routes: DashMap::new() }
    }

    pub fn register(&self, path: &str, handler_id: &str) {
        self.routes.insert(path.to_string(), handler_id.to_string());
    }

    pub fn resolve(&self, path: &str) -> Option<String> {
        self.routes.get(path).map(|v| v.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_resolve() {
        let router = Router::new();
        router.register("/chat", "chat_handler");
        assert_eq!(router.resolve("/chat"), Some("chat_handler".into()));
        assert_eq!(router.resolve("/unknown"), None);
    }
}