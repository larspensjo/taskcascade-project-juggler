#[derive(Debug, PartialEq, Eq)]
pub enum NavigationDecision {
    Allow,
    OpenExternally,
    Block,
}

pub fn classify(url: &url::Url) -> NavigationDecision {
    if matches!(
        url.as_str(),
        "http://taskcascade.localhost/" | "taskcascade://localhost/"
    ) {
        return NavigationDecision::Allow;
    }
    match url.scheme() {
        "http" | "https" if url.host_str() != Some("taskcascade.localhost") => {
            NavigationDecision::OpenExternally
        }
        "mailto" => NavigationDecision::OpenExternally,
        _ => NavigationDecision::Block,
    }
}

pub fn keep_default_context_menu(target_is_editable: bool, has_selection: bool) -> bool {
    target_is_editable || has_selection
}

pub fn url_for_log(url: &str) -> String {
    match url.char_indices().nth(200) {
        Some((end, _)) => format!("{}…", &url[..end]),
        None => url.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_only_the_app_root_document() {
        for url in [
            "http://taskcascade.localhost/",
            "taskcascade://localhost/",
            "HTTP://TaskCascade.localhost/",
            "http://taskcascade.localhost:80/",
        ] {
            assert_eq!(
                classify(&url.parse().unwrap()),
                NavigationDecision::Allow,
                "{url}"
            );
        }
    }

    #[test]
    fn blocks_internal_paths_queries_and_other_schemes() {
        for url in [
            "http://taskcascade.localhost/notes.md",
            "http://taskcascade.localhost/?x",
            "http://taskcascade.localhost:8080/",
            "http://u@taskcascade.localhost/",
            "https://taskcascade.localhost/",
            "http://taskcascade.localhost/#frag",
            "file:///C:/x",
            "javascript:alert(1)",
            "data:text/html,x",
            "ftp://h/x",
            "ms-settings:",
            "taskcascade://localhost/api/health",
        ] {
            assert_eq!(
                classify(&url.parse().unwrap()),
                NavigationDecision::Block,
                "{url}"
            );
        }
    }

    #[test]
    fn opens_web_and_mail_links_externally() {
        for url in [
            "https://example.com/a?b#c",
            "http://example.com",
            "mailto:a@b.c",
        ] {
            assert_eq!(
                classify(&url.parse().unwrap()),
                NavigationDecision::OpenExternally,
                "{url}"
            );
        }
    }

    #[test]
    fn keeps_default_context_menu_only_for_editing_or_selection() {
        for (editable, selection, expected) in [
            (false, false, false),
            (false, true, true),
            (true, false, true),
            (true, true, true),
        ] {
            assert_eq!(
                keep_default_context_menu(editable, selection),
                expected,
                "editable={editable} selection={selection}"
            );
        }
    }

    #[test]
    fn limits_logged_urls_at_character_boundaries() {
        assert_eq!(
            url_for_log("http://taskcascade.localhost/"),
            "http://taskcascade.localhost/"
        );
        let prefix = format!("data:text/plain,{}", "x".repeat(183));
        assert_eq!(url_for_log(&prefix), prefix);
        let prefix = format!("{prefix}é");
        assert_eq!(url_for_log(&prefix), prefix);
        assert_eq!(
            url_for_log(&format!("{prefix}終{}", "x".repeat(1000))),
            format!("{prefix}…")
        );
    }
}
