VAR speaker = "god"
VAR isGaming = false
VAR isWon = false
VAR is_fruit_ninja_done = false
VAR is_goto_diablo = false
VAR is_goto_garden = false

(The scent of scorched wood. The heat of something watching. You descend into the kitchen — or what remains of it.)

+ [Continue] -> intro

=== intro ===
~ speaker = "god"
GHOST KING: Tread lightly. This is the Host’s domain.

* [The Host?] -> desp_host
+ [I’m not afraid.] -> rat_come

=== desp_host ===
GHOST KING: A spirit of flame and fury. Disturb him, and you may never walk these halls again.
+ [Understood.] -> rat_come

=== rat_come ===
~ speaker = "boss"
???: Another trespasser…? Heh. What a delicious surprise.

+ [Who’s there?] -> desp_rat

=== desp_rat ===
~ speaker = "god"
GHOST KING: The Rat Shade… greedy, twisted. He hoards what once was ours. I cannot linger here — his stench burns my essence.

+ [Then I’ll face him alone.] -> game_desp

=== game_desp ===
~ speaker = "boss"
RAT SHADE: Brave words for such a soft belly. You want the memory shard? Then *earn* it. Just don’t wake the Host... or we both burn.

+ [I’ll reclaim what’s mine.] -> game

=== game ===
~ isGaming = true
(The battle begins. Fragments soar through ash and fire. You strike.)

+ [Win] -> Win_game
+ [Fail] -> Lose_game

=== Lose_game ===
~ isGaming = false
~ speaker = "boss"
~ isWon = false
~is_goto_garden = true
RAT SHADE: Hah! Is that all? Maybe your owner deserves to be forgotten.

+ [No… not yet.] -> Result

=== Win_game ===
~ isGaming = false
~ speaker = "boss"
~ isWon = true
~ is_fruit_ninja_done = true
~ is_goto_diablo = true
RAT SHADE: No—! I’ll be dust before the Host returns. Take your cursed piece!

+ [It's mine now.] -> Result

=== Result ===
~ speaker = "god"
{isWon:
    GHOST KING: Well done. The Rat’s grip weakens. While the Host slumbers… we advance.
- else:
    GHOST KING: Even in failure, you endure. Return, recover… and try again.
}

+ [Continue.] -> END
