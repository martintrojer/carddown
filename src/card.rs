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
    static ref TAG_RE: Regex = Regex::new(r"(#[\w-]+)*").unwrap();
    static ref END_OF_CARD_RE: Regex =
        Regex::new(r"^(\s*\-\-\-\s*|\s*\-\s*\-\s*\-\s*|\s*\*\*\*\s*|\s*\*\s*\*\s*\*\s*)$").unwrap();
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct Card {
    pub id: blake3::Hash,
    pub file: PathBuf,
    pub line: u64,
    pub prompt: String,
    pub response: Vec<String>,
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
    let s = line
        .split(&['#', '🧠'])
        .next()
        .context("error stripping tags")?
        .trim();
    Ok(s.trim().to_string())
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
                if prompt.is_empty() {
                    continue;
                }
                let full_answer = caps.get(2).context("error parsing card answer")?.as_str();
                let tags = parse_tags(full_answer);
                cards.push(Card {
                    id: blake3::hash(strip_tags(line)?.as_bytes()),
                    file: PathBuf::from(file),
                    line: line_number as u64,
                    prompt: prompt.to_string(),
                    response: vec![strip_tags(full_answer)?.to_string()],
                    tags,
                });
                state = ParseState::default();
            } else if MULTI_LINE_CARD_RE.is_match(line) {
                let prompt = strip_tags(line)?;
                if prompt.is_empty() {
                    continue;
                }
                state.prompt = Some(prompt.clone());
                state.card_lines.push(prompt);
                state.first_line = Some(line_number as u64);
                state.tags = parse_tags(line);
            }
        } else if END_OF_CARD_RE.is_match(line) && !state.card_lines.is_empty() {
            if let (Some(prompt), Some(line)) = (state.prompt.clone(), state.first_line) {
                let id = blake3::hash(state.card_lines.join("\n").as_bytes());
                let response = state.card_lines.into_iter().skip(1).collect::<Vec<_>>();
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
        assert_eq!(card.response, vec!["42", "and stuff"]);
        assert!(card.tags.is_empty());
        let card = &cards[1];
        assert_eq!(card.line, 4);
        assert_eq!(card.prompt, "q1");
        assert_eq!(card.response, vec!["a1"]);
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
        assert_eq!(card.response, vec!["42"]);
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
        assert_eq!(card.response, vec!["a1"]);
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
            response: vec!["42".to_string()],
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
            response: vec!["42".to_string()],
        };
        let data = serde_json::to_string(&card)?;
        let card2: Card = serde_json::from_str(&data)?;
        assert_eq!(card, card2);
        Ok(())
    }

    #[test]
    fn test_parse_edge_cases() {
        let file = new_md_file().unwrap();

        // Test empty prompt
        let data = "#flashcard\n\na1\n---";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert_eq!(cards.len(), 0);

        // Test empty lines between prompt and response
        let data = "prompt #flashcard\nq1\n\n\na1\n---";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].prompt, "prompt");
        assert_eq!(cards[0].response, vec!["q1", "", "", "a1"]);

        // Test multiple separators
        let data = "prompt#flashcard\nq1\na1\n---\n***\n---";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].response, vec!["q1", "a1"]);

        // Test mixed flashcard markers
        let data = "q1: a1 #flashcard\nq2: a2 🧠";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert_eq!(cards.len(), 2);

        // Test tags with special characters
        let data = "q1: a1 #flashcard #test-123 #test_456 #123test";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert_eq!(cards[0].tags.len(), 3);
    }

    #[test]
    fn test_malformed_cards() {
        let file = new_md_file().unwrap();

        // Test incomplete multiline card (missing separator)
        let data = "prompt #flashcard\nq1\na1";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert!(cards.is_empty());

        // Test malformed one-line card (should be empty because it doesn't match ONE_LINE_CARD_RE)
        let data = ": answer #flashcard";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert!(cards.is_empty());

        // Test empty prompt in multiline card
        let data = "#flashcard\n\na1\n---";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert!(cards.is_empty());
    }

    #[test]
    fn test_unicode_and_special_characters() {
        let file = new_md_file().unwrap();

        // Test unicode characters in prompt and response
        let data = "¿Cómo estás?: Muy bien 🧠 #español";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].prompt, "¿Cómo estás?");
        assert_eq!(cards[0].response, vec!["Muy bien"]);
        assert_eq!(cards[0].tags, HashSet::from(["español".to_string()]));

        // Test emojis and special characters
        let data = "What's your favorite emoji? 🤔 #flashcard\n😊\n⭐\n---";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].prompt, "What's your favorite emoji? 🤔");
        assert_eq!(cards[0].response, vec!["😊", "⭐"]);
    }

    #[test]
    fn test_multiple_cards_same_file() {
        let file = new_md_file().unwrap();
        let data = "\
Q1: A1 #flashcard #tag1
Q2: A2 🧠 #tag2
Q3 #flashcard
multiline
answer
---
Q4: A4 #flashcard";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert_eq!(cards.len(), 4);
        assert_eq!(cards[0].prompt, "Q1");
        assert_eq!(cards[1].prompt, "Q2");
        assert_eq!(cards[2].prompt, "Q3");
        assert_eq!(cards[2].response, vec!["multiline", "answer"]);
        assert_eq!(cards[3].prompt, "Q4");
    }

    #[test]
    fn test_whitespace_handling() {
        let file = new_md_file().unwrap();

        // Test leading/trailing whitespace
        let data = "  Question  :  Answer  #flashcard  #tag1  ";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].prompt, "Question");
        assert_eq!(cards[0].response, vec!["Answer"]);
        assert_eq!(cards[0].tags, HashSet::from(["tag1".to_string()]));

        // Test multiple spaces between elements
        let data = "Q1     :     A1     #flashcard     #tag1";
        fs::write(&file.path(), data).unwrap();
        let cards = parse_file(&file.path()).unwrap();
        assert_eq!(cards[0].prompt, "Q1");
        assert_eq!(cards[0].response, vec!["A1"]);
    }

    #[test]
    fn test_card_separators() {
        let file = new_md_file().unwrap();
        let separators = ["---", "- - -", "***", "* * *"];

        for separator in separators {
            let data = format!(
                "Q1 #flashcard\nA1\n{}\nQ2 #flashcard\nA2\n{}",
                separator, separator
            );
            fs::write(&file.path(), &data).unwrap();
            let cards = parse_file(&file.path()).unwrap();
            assert_eq!(cards.len(), 2);
            assert_eq!(cards[0].prompt, "Q1");
            assert_eq!(cards[1].prompt, "Q2");
        }
    }

    #[test]
    fn test_parse_tags_with_hyphens() {
        let tags = parse_tags("#flashcard #tag-with-hyphen #another-tag");
        assert_eq!(
            tags,
            HashSet::from(["tag-with-hyphen".to_string(), "another-tag".to_string()])
        );
    }

    #[test]
    fn test_strip_tags_whitespace() {
        let line = "  What is the answer?  #flashcard";
        let prompt = strip_tags(line).unwrap();
        assert_eq!(prompt, "What is the answer?");
    }
}
