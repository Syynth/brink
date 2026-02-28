EXTERNAL TestExternal()
EXTERNAL partyLeader()

=== function TestExternal() ===
	INK: test external function called 
	~ return

=== function partyLeader() ===
	~ return "ERR: UNKNOWN FROM INSIDE INK"
	
=== function testInternal() ===
	~ return "hello from inkside"


VAR someExternalString = "hello from another file"
