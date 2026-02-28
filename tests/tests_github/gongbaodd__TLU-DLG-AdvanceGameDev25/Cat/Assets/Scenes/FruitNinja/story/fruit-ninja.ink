VAR speaker = "god"
VAR isGaming = false
VAR isWon = false

Shhhh! This is the hell kitchen.
+ [Okay] -> intro

=== intro ===
~ speaker = "god"
Do not wake up the host here.

* [The host?] -> desp_host
+ [Okay] -> rat_come

=== desp_host ===
The host will cast you out here!
+ [Okay] -> intro

=== rat_come ===
~ speaker = "boss"
Who is there? 
+ [It is me! Who are you?] -> desp_rat

=== desp_rat ===
~ speaker = "god"
This is a rat ghost. He must have one of the pieces. Ohhh, I can't stay for long. I will catch you when you get the piece.

+ [Okay] -> game_desp

=== game_desp ===
~ speaker = "boss"
Haha, you poor cat, want your master's memory piece? Come and get it~ Whatever, you can not make any sound to wake up the host.

+ [I will get my friend back!] -> game

=== game ===
~ isGaming = true
The Game starts

+ [Win] -> Win_game
+ [Fail] -> Lose_game

=== Lose_game ===
~ isGaming = false
~ speaker = "boss"
~ isWon = false
You are so clumsy.

+ [I have to leave.] -> Result

=== Win_game ===
~ isGaming = false
~ speaker = "boss"
~ isWon = true
Noooo! I'd better run before master gets home.

+ He's not home? Great! -> Result

=== Result ===
~ speaker = "god"
{isWon: So good, we can sneak in.} Anyway, let's get back first. 

+ [Okay] -> END

