# Example Ink Story

Once upon a time, there was a brave adventurer standing at a crossroads.

* [Go left into the dark forest]
    -> dark_forest
* [Go right to the sunny village]
    -> sunny_village
* [Stay at the crossroads]
    -> stay

=== dark_forest ===
You venture into the dark forest. The trees are thick and the path is unclear.

A wolf appears before you!

* [Fight the wolf]
    You bravely fight the wolf and emerge victorious!
    -> victory
* [Run away]
    You run as fast as you can back to the crossroads.
    -> END

=== sunny_village ===
You walk towards the sunny village. The people greet you warmly.

The village elder offers you a quest.

* [Accept the quest]
    "Thank you, brave adventurer!" says the elder.
    You set off on a grand adventure!
    -> victory
* [Decline politely]
    You decide to rest in the village instead.
    It's a peaceful day.
    -> END

=== stay ===
You decide to stay at the crossroads, waiting for inspiration.

After a while, you realize that sometimes the best adventure is the one you choose yourself.

-> END

=== victory ===
You have achieved great things!

Your journey has come to a successful end.

-> END
