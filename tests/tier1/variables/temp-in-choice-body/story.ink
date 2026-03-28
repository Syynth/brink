VAR drugged = true
VAR hooper_mentioned = false

-> test_knot

=== test_knot ===
	*	[Talk]
		"There was a young man."
	-	"You seriously entertained that possibility?"
	 * [Yes]
	 	"Yes."
	 * [No]
		"No."

-  "Go on."
	~ temp saved = drugged

	 * [Yes]
		-> DONE
	 * [No]
		{saved:Saved was true.}
		-> DONE
