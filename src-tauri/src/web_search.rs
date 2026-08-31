use url::Url;

use crate::settings::WebSearchEngine;

struct SearchProvider {
    endpoint: &'static str,
    query_key: &'static str,
    title: &'static str,
}

fn provider(engine: WebSearchEngine) -> SearchProvider {
    match engine {
        WebSearchEngine::Bing => SearchProvider {
            endpoint: "https://www.bing.com/search",
            query_key: "q",
            title: "Bing 搜索",
        },
        WebSearchEngine::Baidu => SearchProvider {
            endpoint: "https://www.baidu.com/s",
            query_key: "wd",
            title: "百度搜索",
        },
        WebSearchEngine::Google => SearchProvider {
            endpoint: "https://www.google.com/search",
            query_key: "q",
            title: "Google 搜索",
        },
    }
}

pub(crate) fn search_result_title(engine: WebSearchEngine) -> &'static str {
    provider(engine).title
}

pub(crate) fn search_url(engine: WebSearchEngine, query: &str) -> Option<String> {
    if query.is_empty() || query.contains('\0') {
        return None;
    }
    let provider = provider(engine);
    let mut url = Url::parse(provider.endpoint).ok()?;
    url.query_pairs_mut()
        .clear()
        .append_pair(provider.query_key, query);
    Some(url.to_string())
}

pub(crate) fn open_search(engine: WebSearchEngine, query: &str) -> Result<(), ()> {
    let url = search_url(engine, query).ok_or(())?;
    crate::browser_open::open_url(Url::parse(&url).map_err(|_| ())?)
}

#[cfg(test)]
mod tests {
    use super::{search_result_title, search_url};
    use crate::settings::WebSearchEngine;

    #[test]
    fn provider_urls_preserve_one_encoded_query_value_and_use_fixed_metadata() {
        let query = "我是 Jack & Jill";
        let cases = [
            (
                WebSearchEngine::Bing,
                "Bing 搜索",
                "www.bing.com",
                "/search",
                "q",
            ),
            (
                WebSearchEngine::Baidu,
                "百度搜索",
                "www.baidu.com",
                "/s",
                "wd",
            ),
            (
                WebSearchEngine::Google,
                "Google 搜索",
                "www.google.com",
                "/search",
                "q",
            ),
        ];

        for (engine, title, host, path, key) in cases {
            assert_eq!(search_result_title(engine), title);
            let value = search_url(engine, query).expect("ordinary text should produce a URL");
            let url = url::Url::parse(&value).unwrap();
            assert_eq!(url.scheme(), "https");
            assert_eq!(url.host_str(), Some(host));
            assert_eq!(url.path(), path);
            assert_eq!(
                url.query_pairs()
                    .map(|(key, value)| (key.into_owned(), value.into_owned()))
                    .collect::<Vec<_>>(),
                vec![(key.to_owned(), query.to_owned())]
            );
        }
    }

    #[test]
    fn every_provider_rejects_empty_and_nul_queries() {
        for engine in [
            WebSearchEngine::Bing,
            WebSearchEngine::Baidu,
            WebSearchEngine::Google,
        ] {
            assert_eq!(search_url(engine, ""), None);
            assert_eq!(search_url(engine, "bad\0query"), None);
        }
    }
}
