# carddown demo
# Record with: vhs doc/demo.tape
# Before recording: rm -rf ~/.local/state/carddown
# Requires: carddown on PATH, vhs installed (brew install vhs)

Output doc/demo.gif
Set Width 1000
Set Height 600
Set FontSize 16
Set Shell zsh
Set Theme "Catppuccin Mocha"
Set TypingSpeed 40ms

# --- Show the flashcard source file ---
Type "cat tests/test.md"
Enter
Sleep 3s

# --- Scan for flashcards ---
Type "carddown scan tests/"
Enter
Sleep 2s

# --- Start a revise session ---
Type "carddown revise --maximum-cards-per-session 4"
Enter
Sleep 2s

# First card: reveal then grade
Space
Sleep 2s
Type "5"
Sleep 2s

# Second card
Space
Sleep 2s
Type "4"
Sleep 2s

# Third card
Space
Sleep 2s
Type "5"
Sleep 2s

# Fourth card
Space
Sleep 2s
Type "3"
Sleep 2s

# Back to shell
Type "q"
Sleep 1s

# --- Audit cards ---
Type "carddown audit"
Enter
Sleep 2s

# Navigate
Right
Sleep 1s
Right
Sleep 1s
Right
Sleep 1s

# Quit
Type "q"
Sleep 1s
