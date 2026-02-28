== oldnovel ==

    {
        - currentWorld == "dream" && oldnovel_real:
            -> oldnovel_realvisited
            
        - currentWorld == "dream":
            -> oldnovel_dream
            
        - currentWorld == "real" && oldnovel_dream:
            -> oldnovel_dreamvisited
        
        - else:
            -> oldnovel_real
    }
//---------------- DREAM WORLD ----------------

== oldnovel_dream ==

    Ms Suwan opens the novel, turning to a random page. The scent of the old book prickles her nose. She tries to read the first sentence of the paragraph but her mind wanders.
    
    NOUR: Is that Jane Eyre? I read that book my junior year of high school for AP Lit. Pretty interesting.
    
    :MS SUWAN:
        +[I've never read it] When I first moved to the US I saw a copy of Jane Eyre being sold in a local bookstore and I was taken back because the book was banned in China when I lived there.
    
    NOUR: Wait why was the book banned?

    :MS SUWAN:
        ++[It was a different time] Well during the late 80s, the novel was censored in China due to it being seen by the government as "socially corrupting" to the youth of China during the Cultural Revolution.
        
    NOUR: [writing notes in her journal] oh I did not know that. Thank you for telling me about this. I learned a bit about the Cultural Revolution in school but I never knew about the specifics.
        
    May I ask why you never read the book?
        
    :MS SUWAN:
        +++[Honestly] It wouldn't be right.
    
    Ms Suwan puts the novel back to the shelf, trying to brush off the discontentness shes feeling.

    -> END


== oldnovel_realvisited ==

    Just the same copies of Jane Eyre on the bookshelf
    
    -> END

//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== oldnovel_real ==

    Ms Suwan notices a copy of Jane Eyre sitting vacantly on the bookshelf.
    
    :MS SUWAN:
    
        * Pick it up
            -> pickUpJaneEyre
        * Walk away
            -> walkAway


==pickUpJaneEyre==

    Ms. Suwan picks up the novel. Her fingertips graze the thick dust coating the cover. The cover is faded and the edges are bent.
    
    :MS SUWAN:
        
        * Open it
            -> openJane //shift plane
            
        * Put it back
            -> putItBack
    
    =openJane
    
        &Ms. Suwan opens the book.
        -> DONE


== putItBack ==

    She places the novel back on the shelf, unable to allow herself to remember.
    
    :MS SUWAN:
    
        * Walk away
            -> walkAway


== walkAway==

    Ms. Suwan slowly walks away from the bookshelf, her mind still stuck on the novel.
    
    -> END


== oldnovel_dreamvisited ==

    The Jane Eyre book, sitting alone on the bookshelf.

    -> END

//SUBSEQUENT REAL KNOTS HERE
