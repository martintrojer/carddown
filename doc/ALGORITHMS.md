# Spaced Repetition Algorithms

Carddown implements three spaced repetition algorithms. All use quality grades 0-5, where grades 0-2 are considered failures that reset progress.

## SM2

The classic [SuperMemo 2](https://www.supermemo.com/en/archives1990-2015/english/ol/sm2) algorithm.

- Maintains an ease factor per card (minimum 1.3)
- First successful review: interval = 1 day
- Second successful review: interval = 6 days
- Subsequent: interval = previous interval * ease factor
- Failures reset repetitions and interval to 0

The ease factor adjusts based on quality:

```
EF' = EF + 0.1 - (5 - q) * (0.08 + (5 - q) * 0.02)
```

Higher quality grades increase the ease factor (longer intervals), lower grades decrease it.

## SM5

An enhanced version of SM2 based on [SuperMemo 5](https://www.supermemo.com/en/archives1990-2015/english/ol/sm5).

- Uses an optimal factor matrix indexed by repetition number and ease factor
- Adapts optimal factors based on actual recall performance
- Better at estimating intervals for different difficulty levels
- Falls back to SM2-style ease factor updates when matrix data is sparse

The key improvement over SM2 is that SM5 learns per-difficulty optimal intervals from your review history, rather than using a single formula.

## Simple8

A simplified algorithm with 8 fixed interval stages.

- Uses a fixed progression: 1, 2, 3, 5, 8, 13, 21, 34 days
- Quality determines how many stages to advance or retreat
- No ease factor — purely stage-based
- Simpler to understand and predict

Good for users who prefer predictable, fixed intervals over adaptive scheduling.

## Choosing an algorithm

| Algorithm | Best for |
|---|---|
| **SM5** (default) | Most users — adapts to your performance per card |
| **SM2** | Simpler adaptive scheduling, well-studied |
| **Simple8** | Predictable fixed intervals, easy to reason about |

Select with `--algorithm`:

```bash
carddown revise --algorithm sm2
carddown revise --algorithm sm5
carddown revise --algorithm simple8
```

## Quality grades

All algorithms use the same 0-5 quality scale:

| Grade | Meaning | Effect |
|---|---|---|
| 5 | Perfect recall | Interval increases significantly |
| 4 | Correct with hesitation | Interval increases |
| 3 | Correct with difficulty | Interval increases slightly |
| 2 | Incorrect but seemed easy | **Failure** — interval resets |
| 1 | Incorrect but remembered | **Failure** — interval resets |
| 0 | Complete blackout | **Failure** — interval resets |

A card becomes a **leech** after exceeding the failure threshold (default: 15 failures). Leeches indicate the card should be rewritten or broken into simpler pieces.
