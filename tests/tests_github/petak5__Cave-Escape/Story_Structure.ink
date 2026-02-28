VAR powers = 0

-> background

=== background ===
It has been a looong day in the cave you have stumbled upon. You have been mining those diamonds for some time and even though you have more than enough already it seems like you need more. But since it is getting late you decided to go home.
(Controls)

-> gameplay1

=== gameplay1 ===
The player walks around and collects the diamond strange noises happen and later he finds enemies he has no way to escape and dies.
-> 1stdeath

=== 1stdeath ===
Player wakes up in a beautiful garden and looks upon some eternal creature and scared asks
"Where am I?" -player
"You are in afterlife, you were given the chance to resurrect and try again. But before it I can give you powers that will allow you to kill, do you want them?" - God

+ Take powers
~ powers ++
-> gameplay2
+ Don't get powers
-> gameplay2

=== gameplay2 ===
{ powers > 0:
The player walks around and collects the diamond strange noises happen and later he finds enemies he has no way to escape and dies. Same thing happen just even more enemies
    -else :
The player walks around and collects the diamond strange noises happen and later he finds enemies he has no way to escape and dies.
}
-> 2nddeath


=== 2nddeath ===
Player wakes up again in the beatiful garden and looks upon the eternal creature just asks
"Why am I here again?" -player
"You are in afterlife, you were given the chance to resurrect and try again." - God
{ powers > 0:
"But before it you can keep your powers or i can take them forever?" - God
+ Forfeit powers
~ powers --
-> gameplay3
+ Keep powers
-> gameplay3

    -else :
"But before it I can give you powers that will allow you to kill, do you want them?" - God
+ Take powers
~ powers ++
-> gameplay3
+ Don't get powers
-> gameplay3
}


=== gameplay3 ===
The player walks around and tries to collect the diamond but right before it he heard a voice (probably slimes)
"You really need that" - slimes/diamond
+ Don't collect the diamond
Player doesn't collect the diamonds he doesn't get approach by enemies and is allowed to escape
-> END

+ Collect the diamonds
Player collects the diamond a lot of enemies appear and he dies again.
-> deaths

=== deaths ===
Player wakes up in the garden again
{ powers > 0:
"Yeah yeah, keep power or not" - player
+ Forfeit powers
~ powers --
-> gameplay3
+ Keep powers
-> gameplay3

    -else :
"Do you want powers that will allow you to kill?" - God
+ Take powers
~ powers ++
-> gameplay3
+ Don't get powers
-> gameplay3
}
















