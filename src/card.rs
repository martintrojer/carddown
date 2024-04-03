use anyhow::Context;
use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

lazy_static! {
    static ref TAGS_RE: Regex = Regex::new(r"#flashcard|#spaced|⚡️|🧠").unwrap();
    static ref TAG_RE: Regex = Regex::new(r"(#\w+)*").unwrap();
    static ref ONE_LINE_CARD_RE: Regex = Regex::new(r"^(.*):(.*)").unwrap();
    static ref END_OF_CARD_RE: Regex = Regex::new(r"\-\-\-|\- \- \-|\*\*\*|\* \* \*").unwrap();
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
struct Card {
    id: blake3::Hash,
    file: PathBuf,
    line: u64,
    question: String,
    answer: String,
    tags: Vec<String>, // stripped of hash
}

fn parse_tags(line: &str) -> Vec<String> {
    TAG_RE
        .find_iter(line)
        .map(|m| m.as_str().to_owned())
        .collect()
}

fn parse_file(file: &str) -> Result<Vec<Card>> {
    let contents = std::fs::read_to_string(file)?;
    let mut cards = vec![];
    for (line_number, line) in contents.lines().enumerate() {
        if TAGS_RE.is_match(line) {
            if let Some(caps) = ONE_LINE_CARD_RE.captures(line) {
                let question = caps.get(0).context("error parsing card")?.as_str();
                let full_answer = caps.get(1).context("error parsing card")?.as_str();
                let answer = full_answer.split('#').next().unwrap();
                let tags = parse_tags(full_answer);
                let card = Card {
                    id: blake3::hash(line.as_bytes()),
                    file: PathBuf::from(file),
                    line: line_number as u64 + 1,
                    question: question.to_string(),
                    answer: answer.to_string(),
                    tags,
                };
                cards.push(card);
            }
        }
    }
    Ok(cards)
}

// add some tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_file() -> Result<()> {
        let cards = parse_file("tests/test.md")?;
        assert_eq!(cards.len(), 1);
        let card = &cards[0];
        assert_eq!(card.file.to_str(), Some("tests/test.md"));
        assert_eq!(card.line, 1);
        assert_eq!(
            card.question,
            "What is the answer to life, the universe, and everything?"
        );
        assert_eq!(card.answer, "42");
        assert_eq!(card.tags, vec!["#flashcard", "#spaced", "#test"]);
        Ok(())
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
