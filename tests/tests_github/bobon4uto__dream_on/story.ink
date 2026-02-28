
VAR ENDINGS_FOUND = 0
VAR ALL_ENDINGS = 4
VAR hpan = 0
VAR wolf_counter = 6
-> START
=== START ===

It's night-time.

-> bedroom


=== bedroom ===
 + Go to bed.
 -> bed
 
 * Go to the kitchen. 
 -> kitchen
-> ending

=== kitchen ===
 You spend the whole night in the kitchen, and didn't get any sleep, let alone any dreams.

{ kitchen==1:
 ~ ENDINGS_FOUND++
}
-> ending

=== bed ===
You go to bed and start sleeping.
+ --{~Zzz|zZz|zzZ}--
->dream
=== dream ===
It's daytime.
+ or is it?
-> nightmare
+ It's very bright.
-> collosus
=== nightmare ===
The sun shatters. Darkness falls.
+ It's not that bad.
Wolves howl.
-
+ could be worse?
Your home crumbles.
-
+ now this is bad.
Twe wolves are coming.
-> wolf
===wolf===
~ wolf_counter--
{ wolf_counter != 0:
{wolf_counter} minutes before {~wolves arrive.|hungry wolves will be here.|wolves eat you.|beasts appear.}
}
{ wolf_counter == 0:
# CLEAR
# CLASS: end
# CLASS: danger
They are here.
}
* {wolf_counter != 0}find shelter
nowhere to hide.
* {wolf_counter != 0} run
nowhere to run
+ {wolf_counter != 0}[dream]dream on...
+ {wolf_counter == 0} WAKE UP
->nightmareend

-
-> wolf



===nightmareend==

~ wolf_counter =6
You wake up in cold sweat, the time is 3 AM. It wasn't a dream... it was a nightmare.

{ nightmare==1:
 ~ ENDINGS_FOUND++
}
-> ending

===collosus===
Brighter than any star shines the sphere. it is held by Collossus - the only one who can hold its weight. truly, remarkable sight.

# IMAGE: assets/collossus_far.png
* Isn't it the job of Atlas to hold the sky?
True. But he might be on a vacation, besides, that's not exactly "the sky".
* Help him.
There is no helping him.
+ sus?
not funny.

- 
-> collosus_choice
===collosus_choice===
+ get closer
-> the_way
* observe from here
->beauty
===beauty===
# CLEAR
You watch as the collossal is getting rusty from the rain that comes his way. 
# IMAGE: assets/collossus_far_rain.png
*RUST mentioned????
No, the game is made in INK.
+ Will he withstand that?
Yes, do not worry.
-
Looks like the star that collossus is holding is getting hotter. his hands start to melt.
+ What do I do?
-
think of something cold.
-> cold
===cold===
+ Ice cream
Not cold enough
-> cold
+ {hpan==0}Hot pan
-> hotpan
+ {hpan==1}Very hot pan
-> hotpan
+ {hpan==2}Very VERY hot pan
-> hotpan
+ {hpan==3}Infinitely hot pan
-> hotpan
+ {hpan==4}Coldest pan ever!
-> pan
+ Winter
Almost cold enough.
-> cold
===hotpan ===
~ hpan++

It's hot. not cold.

-> cold

===pan===
You made the pan so hot it overflowed the hotness and became the coldest thing ever, now you use it to cool down the star and help collossus. 
+ toss the pan at the star
The star becomes very cold.
-
+so cold...
You wake up, it's 9 AM, you have a runny nose and your window is wide open.
-
~ ENDINGS_FOUND++
+ so cold...

->ending

===the_way===
You approach the collossus. You realize he is way further than you anticipated. Are you sure you want to approach him?
+yes.
->move
+no.
->collosus_choice
===running===
You run. Collossus isn't even remotely close.
->move
===move===
+ [run]
-> running
+ [walk]
-> walking
+ give up
You stop.
-> collosus_choice

===walking===
You walk. {&You do know that walking is slower than running, right?|Again?|That doesn't seem to help though.|You sure you don't want to just give up?|...|->up_close} 
->move
===up_close===
Somehow, you walked a million steps under an hour. you now see the collossus in front of you. 
# IMAGE: assets/collossus_close.png
-> say
=== say ===
* [hello]
He doesn't speak. There is no reason to greet him. 

* [bye]
You didn't greet him.

+ Can I help you? 
-> help_ending

-
-> say
=== help_ending ===
You help the collossus by taking the weight of the star off his shoulders and substituting him. 
+ So heavy...
Just as you think that - you wake up. It's 5 AM, and your cat is sleeping on you.
- 
+ So heavy...
{ help_ending==1:
 ~ ENDINGS_FOUND++
}
-> ending









=== ending
- You found {ENDINGS_FOUND}/{ALL_ENDINGS} endings.
 + {ENDINGS_FOUND<ALL_ENDINGS} Restart
    # CLEAR
    -> START
 + [Finish playing]
 # CLASS end
 THE END
    -> END
