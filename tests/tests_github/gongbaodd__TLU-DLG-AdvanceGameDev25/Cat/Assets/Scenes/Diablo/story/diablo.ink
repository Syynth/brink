VAR isGaming = false
VAR speaker = "god"

This is where they hide their treasure.
+ [Okay] -> intro

=== intro ===
They must have hid the last piece in one of the chests.
+ [That is easy] -> monsters

=== monsters ===
But be careful, there are guards and not all may look like as it is.
+ [Okay] -> game

===game====
~ isGaming = true
The Game starts
+ [Win] -> ending
+ [Lose] -> ending
===ending===
~ isGaming = false
Let's go back!

+ Okay -> END
