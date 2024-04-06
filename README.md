# CARDDOWN

CARDDOWN is a simple cli tool to keep track of (and study) flashcards in text files.

## Features

  - Support many cards in a single file.
  - Keeps tracks of flashcard by hash, thus supports file being edited and cards moved around.
  - Supports tags, to filter your study sessions.
  - Supports multiple spaced-repetition algorithms.
  - No dependencies, fast, database in json format.
  
## Rationale

- I have a lot of notes in markdown, some of them contains facts that I want to be able to recall easily.
- I want to use a cli tool for drilling these facts that is simple and easy to use and install. 
- I want to easily mark these facts in my notes (turn them into flashcards).
- I want a single markdown file to contain an arbitrary number of flashcards.
- I edit my notes often, and move stuff around within and between files.
- I want to the tool to keep track of the cards I want to practice, even when they move.
- I want to be able to constrain what to practice via tags.
- I want to the tool to keep track of my progress and adjust the practice schedule accordingly.
- I want to explore different spaced repetition algorithms.

## Acknowledgements

- [NeuraCache Markdown Flashcard Specification](https://github.com/NeuraCache/markdown-flashcards-spaced-repetition)
- [Emacs org-drill mode](https://gitlab.com/phillord/org-drill/)
