VAR soulLeft = 2

It is a dark night. A guy walking a cat on the street. Suddenly, he saw a new garden.

 * [Strange, never saw this garden before.] The guy goes to the garden. 

Then there is a strong wind blowing to him. Then he faint.
-> Gardern


=== Gardern ===
The cat goes to the guy. 

* [Looks I am finally free!] -> leave
* I have to save him, The cat thinks. 
-> CatMaster

= leave
The cat tries to get out of the garden. No matter how hard he tries, he goes back to his owner.
-> Gardern

=== CatMaster ===
Then a cat ghost shows up, "Look like a nice kitty got troubled."

+ [No, I am fine.] -> CatMaster
+ [Yes, please help me save my friend.] -> StoryIntro

=== StoryIntro ===
{soulLeft > 0: There are three evil ghosts here, one is a rat the other one is a boy. The collect souls for fun. You have to find them and get all the soul pieces back to your friend. | You get all the soul pieces! You can save your friend now. }


* Go to Hell Kitchen, to find the mouse -> HellKitchen
* Go to Dungeon, to find the boy -> Dungeon
* -> Ending

=== HellKitchen ===
The rat King laughs, "Your friend's soul is here, come and catch it." Then he throw stuff to the cat. catch all of them except the bomb.

+ [catch bomb] -> Fail
+ [catch soul] -> Win

= Fail
The rat King laughs, "Little kitty, go back and cry for your friend." -> HellKitchen

= Win
The rat King, "You are just lucky this time!" 
~ soulLeft -= 1
-> StoryIntro

=== Dungeon ===
A boy's sound "You will never find what I hide!"

+ [Find the soul] -> Win

= Win
~ soulLeft -= 1
-> StoryIntro

=== Ending ====
The guy wakes up when all the soul pieces get back to him.

-> END
