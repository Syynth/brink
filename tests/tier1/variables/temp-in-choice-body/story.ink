VAR drugged = true
VAR hooper_mentioned = false
VAR forceful = 0
VAR evasive = 0
VAR revealedhooperasculprit = false

-> i_met_a_young_man

=== function raise(ref x)
 	~ x = x + 1

=== i_met_a_young_man
	*	[Talk]
		"There was a young man."
	-	Harris is not letting me off any more.
		"You seriously entertained that possibility?"
	 * [Yes]
	 	"Yes, I considered it. <>
	 * [No]
		"No. Not for more than a moment." <>
	* [Lie]
		"I was quite certain. <>
- 	He seemed to know all about me."
	The way Harris is staring I expect him to strike me.

	 *  [Yes] "It's a lonely life in this place."
		"That's how it is in the Service," Harris answers.
		* *	[Argue] "I'm not in the Service."
			Harris shakes his head. "Yes, you are."
		* * [Agree] "Perhaps. But I didn't choose this life."
			Harris shakes his head. "No."
		- - Then he waves the thought aside.

	 * (nope) { not drugged  }  [No] "The boy was a pretty simpleton."
			 ~ raise(evasive)
			Harris doesn't flinch.

	 * { drugged  }   		[No]
	 	"It wasn't," I reply.
	 	He simply nods.
	 * { not drugged  }   	[Lie] -> nope

-  "Go on with your confession."
- (paused)
	 { not nope:
		That gives me pause.
	}
	"This young man was blackmailing you over your affair?"

	~ temp harris_thinks_youre_drugged = drugged

	 { drugged:
	 	~ drugged = false
		Whatever it was they put in my drink is wearing off.
	}

	 * (yes) [Yes]
	 	"Yes. I suppose he was their agent."
		-> DONE
	 * (notright) [No]
	 	"No, the young man wasn't blackmailing me."
		{ not hooper_mentioned:
			"Hooper!" {harris_thinks_youre_drugged:He does not doubt me for a moment.}
		- else:
			"Now look here," Harris interrupts.
		}
		 ~ revealedhooperasculprit = true
		-> DONE
	 * [Tell the truth] 	-> yes
	 * [Lie] 				-> notright
