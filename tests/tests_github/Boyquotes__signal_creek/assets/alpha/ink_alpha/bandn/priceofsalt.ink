== priceofsalt ==

    {
        - currentWorld == "dream" && priceofsalt_real:
            -> priceofsalt_realvisited
            
        - currentWorld == "dream":
            -> priceofsalt_dream
            
        - currentWorld == "real" && priceofsalt_dream:
            -> priceofsalt_dreamvisited
        
        - else:
            -> priceofsalt_real
    }
//---------------- DREAM WORLD ----------------

== priceofsalt_dream ==

    //there's an overflowing shelf of books, all with the same title and author: The Price of Salt by Patricia Highsmith.
    //Nour used to read bits and pieces of the book whenever she'd come to the mall with her family; they never let her buy it, so she'd read as many chapters as she could whenever they'd leave her be in the store.
    
    Nour finds a massive stack of books. They're all copies of the same novel, titled The Price of Salt.
    
    NOUR: Whoa, never seen so many copies of these! There's always only one or two left in the store at any given time.
    
    NOUR: I used to hide one with a bookmark on one of the shelves so I'd always know where it was and where I left off when I'd come back to read it. I was so scared someone would buy it.
    
    NOUR: Good times.
    
    NICK: Why didn't you just like, y'know, buy a copy?
    
    NOUR: It's, uh. It's a queer book. My parents would've killed me if they saw me with it, so I'd come here while they were busy shopping in the mall to read it.
    
    NICK: Huh. Very sneaky!
    
    -> END


== priceofsalt_realvisited ==
    
    NOUR: Oh, *now* I know what book that was in the other place! It's the Price of Salt. I've never seen so many copies of these.
    
    NOUR: Guess the folks here really like the book.
    
    NICK: Urgh, looks boring to me.
    
    NOUR: Well, I really liked it. I used to hide one with a bookmark on one of the shelves so I'd always know where it was and where I left off when I'd come back to read it. I was so scared someone would buy it.
    
    NOUR: Good times.
    
    NICK: Why didn't you just like, y'know, buy a copy?
    
    NOUR: It's, uh. It's a queer book. My parents would've killed me if they saw me with it, so I'd come here while they were busy shopping in the mall to read it.
    
    NICK: Huh. Sneaky!
    
    -> END


//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== priceofsalt_real ==

    //the real price of salt file
    //there's a single copy of The Price of Salt by Patricia Highsmith hidden amongst a bunch of other books, none belonging to any one genre.
    
    NOUR: I can hardly read the title on this, it's so beaten up. Who treats a book like that?!
    
    NOUR: Looks kinda familiar, but it's too hard to make out.
    
    -> END


== priceofsalt_dreamvisited ==

    NOUR: Oh, there's a copy of The Price of Salt here, too. It's so beaten up, it's hard to tell what book it is.
    
    NOUR: I almost want to take it for old time's sake.
     
    -> END


//SUBSEQUENT REAL KNOTS HERE
