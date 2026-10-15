# Game rules

This file describes the game that is currently plugged into the template.
Today that is the **example game** (`src/example.rs`), a placeholder that
demonstrates the whole flow without being a real game. When you build your
own game, replace this content with its rules.

## Example game

- The screen shows "Your game here", the level number (top left) and the
  score (top right).
- A dot circles a paw image (`assets/paw.png`) in the middle of the screen
  once every 3 seconds of game time. It shows that the game advances with
  the fixed time step, stops while paused and moves smoothly in recordings.
- **Space** gives one point and plays the framework's blip; every fifth
  point plays the game's own sound (`assets/chime.wav`) instead. Both are
  part of the video's sound track.
- **Enter** or the **Finish level** button finishes the level. The button
  lights up under the mouse. The results screen then shows the level, the
  score and the played time.
- Each "Next round" starts the next level with the score at 0. "Play" in the
  main menu always starts at level 1.
- There is no way to lose; a round ends only by finishing the level or by
  aborting it from the pause menu.

## Framework behaviour every game gets

These are provided by the framework and need no code in the game:

- **Esc** or **P** pauses; the pause menu offers Resume and Abort round.
  While paused, the game receives no input and no updates.
- The results screen shows `RoundResult::score` from `Game::finished` and
  the played (unpaused) time.
- Recording, saving and discarding videos.

## Your game's rules

*Describe your game here: goal, controls, scoring, how a level ends, levels
and difficulty, and any numbers a player or developer needs to know.*
