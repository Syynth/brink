== kitchen

<-inv( ->kitchen )


+ [LOOK]
The kitchen looked like any other restaurant kitchen.
Counters. Utensils. Shelves. A Fridge.
A large shelf was full of various syrups.
{Knowledge !? phonecall:
    A young man was doing the dishes at a large sink.
- else:
    Dan wasn't here right now. This was my chance to examine that sink.
}
On the back wall a door led inside a big walk-in freezer.

+ [TALK TO DISHWASHER\|TALK TO DAN\|TALK TO DANIEL\|TALK TO MAN]
{Knowledge !? phonecall:
    ->dishwasher
- else:
    He's not here right now.
}

+ [LOOK AT COUNTER\|LOOK AT COUNTERS]
The counters looked clean and tidy.
No ice cream was being made here today.

+ [LOOK AT UTENSILS\|LOOK AT UTENSIL]
I don't know what these things do.
I can barely fry an egg.
I should look at something else.

+ [LOOK AT FRIDGE]
I peeked inside the fridge.
It was stocked with eggs, cream and other ingredients.
Nothing was out of the ordinary.
I closed the fridge again.

+ [LOOK AT SHELF\|LOOK AT SHELVES\|LOOK AT SYRUPS]
The wall was covered with various ice cream syrups.
It looked like they were made here.
The labels had no names, only numbers.

+ [TAKE SYRUP]
I had no use for it, so I decided not to.

+ [LOOK AT DISHWASHER\|LOOK AT MAN\|LOOK AT DAN\|LOOK AT DANIEL]
{Knowledge !? phonecall:
    A young man washing dishes.
    What a strange thing to do at a crime scene.
    I guess the cop told him it was okay.
- else:
    He's not here right now. This is my chance to check out that soap.
}

+ (sink) [LOOK AT SINK]
A large sink, full of dishwater and dirty dishes.
A tea towel, a bar of soap and a bottle of washing-up liquid are on the counter next to the sink.

+ {sink} [LOOK AT TEA TOWEL\|LOOK AT TOWEL]
It's for drying dishes. Probably.

+ {sink} [LOOK AT SOAP]
{Knowledge ? phonecall:
    I should pick it up to examine it closer.
- else:
    It looks like a regular bar of soap.
}

+ {sink || Knowledge ? phonecall} [TAKE SOAP]
{Knowledge ? phonecall: 
    ->climax
- else:
    I see no reason to pick up a bar of soap.
}

+ {sink} [LOOK AT LIQUID\|LOOK AT BOTTLE\|LOOK AT WASHING UP LIQUID]
It's just regular washing-up liquid.

+ [LOOK AT FREEZER]
There was a large walk-in freezer in the back of the room.
The door appeared to be lockable, but unlocked.

+ [GO TO RESTAURANT\|LEAVE]
I walked back to the restaurant.
->restaurant

+ [GO TO FREEZER\|ENTER FREEZER]
I walked inside the freezer.
->freezer

- ->kitchen