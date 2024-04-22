use anyhow::Context;
use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

lazy_static! {
    static ref CARD_RE: Regex = Regex::new(r"#flashcard|🧠").unwrap();
    static ref ONE_LINE_CARD_RE: Regex = Regex::new(r"^(.*):(.*)").unwrap();
    static ref MULTI_LINE_CARD_RE: Regex = Regex::new(r"#flashcard").unwrap();
    static ref TAG_RE: Regex = Regex::new(r"(#\w+)*").unwrap();
    static ref END_OF_CARD_RE: Regex = Regex::new(r"^(\-\-\-|\- \- \-|\*\*\*|\* \* \*)$").unwrap();
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct Card {
    pub id: blake3::Hash,
    pub file: PathBuf,
    pub line: u64,
    pub prompt: String,
    pub response: String,
    pub tags: HashSet<String>,
}

fn parse_tags(line: &str) -> HashSet<String> {
    let mut tags: HashSet<String> = TAG_RE
        .find_iter(line)
        .map(|m| m.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s[1..s.len()].to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    tags.remove("flashcard");
    tags
}

fn strip_tags(line: &str) -> Result<String> {
    Ok(line
        .split(&['#', '🧠'])
        .next()
        .context("error stripping tags")?
        .trim()
        .to_string())
}

#[derive(Debug, Default)]
struct ParseState {
    card_lines: Vec<String>,
    tags: HashSet<String>,
    prompt: Option<String>,
    first_line: Option<u64>,
}

pub fn parse_file(file: &Path) -> Result<Vec<Card>> {
    let contents =
        fs::read_to_string(file).with_context(|| format!("Error reading `{}`", file.display()))?;
    if contents.contains("@carddown-ignore") {
        log::info!("ignoring file: {}", file.display());
        return Ok(vec![]);
    }
    let mut cards = vec![];
    let mut state = ParseState::default();
    for (line_number, line) in contents.lines().enumerate() {
        log::debug!("line_number: {}, line: {}", line_number, line);
        log::debug!(
            "first_line: {:?}, card_lines: {:?}",
            state.first_line,
            state.card_lines
        );
        if CARD_RE.is_match(line) {
            if let Some(caps) = ONE_LINE_CARD_RE.captures(line) {
                log::debug!("caps: {:?}", caps);
                let prompt = caps
                    .get(1)
                    .context("error parsing card prompt")?
                    .as_str()
                    .trim();
                let full_answer = caps.get(2).context("error parsing card answer")?.as_str();
                let tags = parse_tags(full_answer);
                cards.push(Card {
                    id: blake3::hash(strip_tags(line)?.as_bytes()),
                    file: PathBuf::from(file),
                    line: line_number as u64,
                    prompt: prompt.to_string(),
                    response: strip_tags(full_answer)?.to_string(),
                    tags,
                });
                state = ParseState::default();
            } else if MULTI_LINE_CARD_RE.is_match(line) {
                let prompt = strip_tags(line)?;
                state.prompt = Some(prompt.clone());
                state.card_lines.push(prompt);
                state.first_line = Some(line_number as u64);
                state.tags = parse_tags(line);
            }
        } else if END_OF_CARD_RE.is_match(line) && !state.card_lines.is_empty() {
            if let (Some(prompt), Some(line)) = (state.prompt.clone(), state.first_line) {
                let id = blake3::hash(state.card_lines.join("\n").as_bytes());
                let response = state
                    .card_lines
                    .into_iter()
                    .skip(1)
                    .collect::<Vec<_>>()
                    .join("\n");
                cards.push(Card {
                    id,
                    file: PathBuf::from(file),
                    line,
                    prompt,
                    response,
                    tags: state.tags,
                });
                state = ParseState::default();
            }
        } else if !state.card_lines.is_empty() {
            state.card_lines.push(line.to_string());
        }
    }
    Ok(cards)
}

// add some tests
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{self, NamedTempFile};

    fn new_md_file() -> Result<NamedTempFile> {
        tempfile::Builder::new()
            .suffix(".md")
            .tempfile()
            .context("error creating tempfile")
    }

    #[test]
    fn test_parse_multi_line_cards() {
        let file = new_md_file().unwrap();
        let data =
            "What is the answer to life, the universe, and everything? #flashcard\n42\nand stuff\n---\n             q1:a1 🧠 ";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert_eq!(cards.len(), 2);
        let card = &cards[0];
        assert_eq!(
            card.file.to_str(),
            Some(format!("{}", file.path().display()).as_str())
        );
        assert_eq!(card.line, 0);
        assert_eq!(
            card.prompt,
            "What is the answer to life, the universe, and everything?"
        );
        assert_eq!(card.response, "42\nand stuff");
        assert!(card.tags.is_empty());
        let card = &cards[1];
        assert_eq!(card.line, 4);
        assert_eq!(card.prompt, "q1");
        assert_eq!(card.response, "a1");
        assert!(card.tags.is_empty());
    }

    #[test]
    fn test_parse_file() {
        let file = new_md_file().unwrap();
        let data =
            "What is the answer to life, the universe, and everything?: 42 #flashcard #foo #test";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert_eq!(cards.len(), 1);
        let card = &cards[0];
        assert_eq!(
            card.file.to_str(),
            Some(format!("{}", file.path().display()).as_str())
        );
        assert_eq!(card.line, 0);
        assert_eq!(
            card.prompt,
            "What is the answer to life, the universe, and everything?"
        );
        assert_eq!(card.response, "42");
        assert_eq!(
            card.tags,
            HashSet::from(["foo".to_string(), "test".to_string()])
        );

        let data = "q1:a1 🧠 ";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert_eq!(cards.len(), 1);
        let card = &cards[0];
        assert_eq!(card.line, 0);
        assert_eq!(card.prompt, "q1");
        assert_eq!(card.response, "a1");
        assert!(card.tags.is_empty());

        let data = "";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert!(cards.is_empty());

        let data = " hello : there";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert!(cards.is_empty());

        let data = "#flashcard\nq1\na1\n#flashcard\nq2\na2\n-";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert!(cards.is_empty());
    }

    #[test]
    fn test_parse_file_with_ignore() {
        let file = new_md_file().unwrap();
        let data = "@carddown-ignore\nWhat is the answer to life, the universe, and everything?: 42 #flashcard #foo #test";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert!(cards.is_empty());
    }

    #[test]
    fn test_strip_tags() {
        let line =
            "What is the answer to life, the universe, and everything? #flashcard #foo #test";
        let prompt = strip_tags(line).unwrap();
        assert_eq!(
            prompt,
            "What is the answer to life, the universe, and everything?"
        );

        let line = "What is the answer to life, the universe, and everything? 🧠 #foo #test";
        let prompt = strip_tags(line).unwrap();
        assert_eq!(
            prompt,
            "What is the answer to life, the universe, and everything?"
        );
    }

    #[test]
    fn test_parse_tags() {
        let tags = parse_tags("#flashcard #spaced #test # ##");
        assert_eq!(
            tags,
            HashSet::from(["spaced".to_string(), "test".to_string()])
        );
    }

    #[test]
    fn test_card() {
        let card = Card {
            id: blake3::hash(b"test"),
            file: PathBuf::from("test.rs"),
            line: 42,
            tags: HashSet::new(),
            prompt: "What is the answer to life, the universe, and everything?".to_string(),
            response: "42".to_string(),
        };
        assert_eq!(card.file.to_str(), Some("test.rs"));
        assert_eq!(card.line, 42);
        assert_eq!(card.id.to_string().len(), 64);
    }

    #[test]
    fn test_serde() -> Result<()> {
        let card = Card {
            id: blake3::hash(b"test"),
            file: PathBuf::from("test.rs"),
            line: 42,
            tags: HashSet::from(["test".to_string()]),
            prompt: "What is the answer to life, the universe, and everything?".to_string(),
            response: "42".to_string(),
        };
        let ron = ron::to_string(&card)?;
        let card2: Card = ron::from_str(&ron)?;
        assert_eq!(card, card2);
        Ok(())
    }
}
