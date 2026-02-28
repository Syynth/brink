VAR isGaming = false
VAR speaker = "god"
VAR is_won = false
VAR is_goto_garden = false

(The shadows are thicker here. The air smells of rust and secrets. You've made it — to the heart of the Ghost King's lair.)

+ [Continue] -> intro

=== intro ===
~ speaker = "god"
GHOST KING: You clawed your way through every illusion, every minion... and now you stand before my vault?

+ [Give him back.] -> monsters

=== monsters ===
GHOST KING: You’re bold, little flame. But not everything here is what it seems. Some chests hold truth. Others — teeth.

+ [I’m not afraid of your tricks.] -> game

=== game ===
~ isGaming = true
(The final trial. The stolen memories whisper from within the chests. But the Ghost King has twisted them — only one holds what you seek.)

+ [I found it.] -> Win_game
+ [I failed…] -> Lose_game

=== Win_game ===
~ isGaming = false
~ speaker = "god"
~ is_won = true
GHOST KING: Impossible... You’ve undone what I shattered.

+ [I'm taking him back.] -> ending

=== Lose_game ===
~ isGaming = false
~ speaker = "god"
~ is_goto_garden = true
GHOST KING: Hahaha! Futile! There goes all hope of seeing your master again.

+ [I won’t give up.] -> ending

=== ending ===
{is_won:
    (The final memory glows in your paws. It hums with warmth — a promise of return.)
 - else:
    (The chests vanish in smoke. But somewhere in the dark, the final memory still calls to you.)
 }

 + [Escape] -> END
