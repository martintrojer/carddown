use anyhow::Context;
use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
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
    pub question: String,
    pub answer: String,
    pub tags: Vec<String>,
}

fn parse_tags(line: &str) -> Vec<String> {
    TAG_RE
        .find_iter(line)
        .map(|m| m.as_str().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

fn strip_tags(line: &str) -> Result<String> {
    Ok(line
        .split(&['#', '🧠'])
        .next()
        .context("error stripping tags")?
        .trim()
        .to_string())
}

pub fn parse_file(file: &Path) -> Result<Vec<Card>> {
    let contents =
        fs::read_to_string(file).with_context(|| format!("Error reading `{}`", file.display()))?;
    let mut cards = vec![];
    let mut card_lines: Vec<String> = vec![];
    let mut tags: Vec<String> = vec![];
    let mut question: Option<String> = None;
    let mut first_line: Option<u64> = None;
    for (line_number, line) in contents.lines().enumerate() {
        log::debug!("line_number: {}, line: {}", line_number, line);
        log::debug!("first_line: {:?}, card_lines: {:?}", first_line, card_lines);
        if CARD_RE.is_match(line) {
            if let Some(caps) = ONE_LINE_CARD_RE.captures(line) {
                log::debug!("caps: {:?}", caps);
                let question = caps
                    .get(1)
                    .context("error parsing card question")?
                    .as_str()
                    .trim();
                let full_answer = caps.get(2).context("error parsing card answer")?.as_str();
                let tags = parse_tags(full_answer);
                cards.push(Card {
                    id: blake3::hash(line.as_bytes()),
                    file: PathBuf::from(file),
                    line: line_number as u64,
                    question: question.to_string(),
                    answer: strip_tags(full_answer)?.to_string(),
                    tags,
                });
            } else if MULTI_LINE_CARD_RE.is_match(line) {
                question = Some(strip_tags(line)?);
                card_lines.push(line.to_string());
                first_line = Some(line_number as u64);
                tags = parse_tags(line);
            }
        } else if END_OF_CARD_RE.is_match(line) && !card_lines.is_empty() {
            if let (Some(quest), Some(line)) = (question.clone(), first_line) {
                let id = blake3::hash(card_lines.join("\n").as_bytes());
                let answer = card_lines
                    .into_iter()
                    .skip(1)
                    .collect::<Vec<_>>()
                    .join("\n");
                cards.push(Card {
                    id,
                    file: PathBuf::from(file),
                    line,
                    question: quest,
                    answer,
                    tags,
                });
                first_line = None;
                question = None;
                card_lines = Vec::new();
                tags = Vec::new();
            }
        } else if !card_lines.is_empty() {
            card_lines.push(line.to_string());
        }
    }
    Ok(cards)
}

// add some tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_multi_line_cards() {
        let data =
            "What is the answer to life, the universe, and everything? #flashcard\n42\nand stuff\n---\n             q1:a1 🧠 ";
        fs::write("/tmp/test2.md", data).unwrap();
        let cards = parse_file(&PathBuf::from("/tmp/test2.md")).unwrap();
        assert_eq!(cards.len(), 2);
        let card = &cards[0];
        assert_eq!(card.file.to_str(), Some("/tmp/test2.md"));
        assert_eq!(card.line, 0);
        assert_eq!(
            card.question,
            "What is the answer to life, the universe, and everything?"
        );
        assert_eq!(card.answer, "42\nand stuff");
        assert_eq!(card.tags, vec!["#flashcard"]);
        let card = &cards[1];
        assert_eq!(card.line, 4);
        assert_eq!(card.question, "q1");
        assert_eq!(card.answer, "a1");
        assert!(card.tags.is_empty());
    }

    #[test]
    fn test_parse_file() {
        let data =
            "What is the answer to life, the universe, and everything?: 42 #flashcard #foo #test";
        fs::write("/tmp/test.md", data).unwrap();
        let cards = parse_file(&PathBuf::from("/tmp/test.md")).unwrap();
        assert_eq!(cards.len(), 1);
        let card = &cards[0];
        assert_eq!(card.file.to_str(), Some("/tmp/test.md"));
        assert_eq!(card.line, 0);
        assert_eq!(
            card.question,
            "What is the answer to life, the universe, and everything?"
        );
        assert_eq!(card.answer, "42");
        assert_eq!(card.tags, vec!["#flashcard", "#foo", "#test"]);

        let data = "q1:a1 🧠 ";
        fs::write("/tmp/test.md", data).unwrap();
        let cards = parse_file(&PathBuf::from("/tmp/test.md")).unwrap();
        assert_eq!(cards.len(), 1);
        let card = &cards[0];
        assert_eq!(card.line, 0);
        assert_eq!(card.question, "q1");
        assert_eq!(card.answer, "a1");
        assert!(card.tags.is_empty());

        let data = "";
        fs::write("/tmp/test.md", data).unwrap();
        let cards = parse_file(&PathBuf::from("/tmp/test.md")).unwrap();
        assert!(cards.is_empty());

        let data = " hello : there";
        fs::write("/tmp/test.md", data).unwrap();
        let cards = parse_file(&PathBuf::from("/tmp/test.md")).unwrap();
        assert!(cards.is_empty());

        let data = "#flashcard\nq1\na1\n#flashcard\nq2\na2\n-";
        fs::write("/tmp/test.md", data).unwrap();
        let cards = parse_file(&PathBuf::from("/tmp/test.md")).unwrap();
        assert!(cards.is_empty());
    }

    #[test]
    fn test_parse_tags() {
        let tags = parse_tags("#flashcard #spaced #test");
        assert_eq!(tags, vec!["#flashcard", "#spaced", "#test"]);
    }

    #[test]
    fn test_card() {
        let card = Card {
            id: blake3::hash(b"test"),
            file: PathBuf::from("test.rs"),
            line: 42,
            tags: vec![],
            question: "What is the answer to life, the universe, and everything?".to_string(),
            answer: "42".to_string(),
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
            tags: vec!["test".to_string()],
            question: "What is the answer to life, the universe, and everything?".to_string(),
            answer: "42".to_string(),
        };
        let json = serde_json::to_string(&card)?;
        let card2: Card = serde_json::from_str(&json)?;
        assert_eq!(card, card2);
        Ok(())
    }
}
