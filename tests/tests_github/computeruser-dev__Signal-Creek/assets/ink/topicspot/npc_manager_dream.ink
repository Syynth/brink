// REAL NOT VISITED
// here, we check if real world version of the manager has been visited, and skip to realvisited if so.
The store manager is next to one of the shirt racks, and as the party approaches, Nick visibly gets nervous. 

He glances away from the manager quickly when he looks over.

MANAGER: "Hello, can I help you guys?"

:PARTY:
* NOUR: "says something" -> NourSpeaks1
* MS.SUWAN: "says something" -> MsSuwanSpeaks1
* NICK: "..." -> NickSpeaks1

===NourSpeaks1===
Nour asks something but its not important to Nick's plot. They have a nice conversation about why the mall is closing or something, and then end the conversation.
-> END

===MsSuwanSpeaks1===
Ms Suwan asks something but its not important to Nick's plot. They have a nice conversation, if a bit awkward because Ms.Suwan doesn't care for TopicSpot, and then end the conversation.
->END

===NickSpeaks1===
Nick is afraid to talk, and so doesn't say anything, and lets the others speak. 
-> END


===realvisited===
// Here we will have to check booleans of whether or not Nick interacted with certain objects, so that in the real world he has something to talk about with the Manager that advances his plot.

The store manager is next to one of the shirt racks, and as the party approaches, Nick visibly gets nervous. 

He glances away from the manager quickly when he looks over.

MANAGER: "Welcome back! Find anything new I can help you with?"

:PARTY:
* NOUR: "says something" -> NourSpeaks2
* MS.SUWAN: "says something" -> MsSuwanSpeaks2
* NICK: "..." -> NickSpeaks1
// if he has the shirt, go to NickSpeaks2

===NourSpeaks2===
Nour asks something but its not important to Nick's plot. They have a nice conversation about why the mall is closing or something, and then end the conversation.
-> END
===MsSuwanSpeaks2===
Ms Suwan asks something but its not important to Nick's plot. They have a nice conversation, if a bit awkward because Ms.Suwan doesn't care for TopicSpot, and then end the conversation.
-> END
===NickSpeaks2===
Nick shows the shirt to the manager. He's so intimidated by him despite the fact that he isn't actually doing anything intimidating, that when he's told that he can't have the shirt because he needs to pay for it, he shifts back to the real world.
-> END