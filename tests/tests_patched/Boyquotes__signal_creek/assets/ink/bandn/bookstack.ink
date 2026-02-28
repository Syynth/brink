== bookstack ==

    It's a shelf filled with books.
    
        
    {
        - bookstack1:
            -> bookstackvisited
        - else:
            -> bookstack1

    }
    
    == bookstack1 ==
    
    NOUR: "Don't exactly have the leisure to be reading right now, but sure."
    
    MS. SUWAN: "Agreed. No use in wasting our time looking at books. We should be focusing on getting out of here."
    
    NICK: "Come on, guys. What if there are clues?"
    
    NOUR: "Clues...?"
    
    NICK: "Yeah, like..."
    
    NICK: "When you pull on a book and the whole shelf, like, swings out and there's a hidden exit behind it!"
    
    MS. SUWAN: "You watch too many movies."
    
    MS. SUWAN: "This is a commercial bookstore—not some mysterious billionaire's private library."
    
    MS. SUWAN: "I doubt there would be <i>any</i> clues, nor hidden exits, behind a bookstore wall."
    
    Nick eagerly tugs on every book.
    
    NICK: "Trust me, guys. I'll find something!"
    
    NOUR: "...All right, Nick, you do that."
    -> END
    
    == bookstackvisited ==
    
    NICK: "Aw jeez... looks like there isn't a hidden exit after all..."
    
    NOUR: "It was, um, worth a try?"
    
    -> END
