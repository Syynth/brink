//the real old novel file

//in the real world the old novel has an effect on Ms Suwan and 
->MsSuwanOldNovel
->PartyOldNovel

==MsSuwanOldNovel==
Ms Suwan notices a copy of Jane Eyre sitting vacantly on the bookshelf. 
    * Pick it up -> pickUpJaneEyre
    * Walk away -> walkAway

==pickUpJaneEyre==

She picks up the novel. Her fingertips graze the thick dust coating the cover. The cover is faded and the edges are bent. 
    * Open it ->DONE //shift plane
    * Put it back -> putItBack

== putItBack ==

She places the novel back on the shelf, unable to allow herself to remember.
    *Walk away-> walkAway

== walkAway==
    She slowly walks away from the bookshelf, her mind still stuck on the novel.
    ->END

==PartyOldNovel==
A dusted copy of Jane Eyre sits on the bookshelf.
->DONE
    


