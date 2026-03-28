VAR drugged = true

-> test_knot

=== test_knot ===
	 * [A]
		"A."
-
	~ temp saved = drugged
	 * [Yes]
		-> DONE
	 * [No]
		{saved:Saved was true.}
		-> DONE
