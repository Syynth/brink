== dragonplush ==

    {
        - currentWorld == "dream" && dragonplush_real:
            -> dragonplush_realvisited
            
        - currentWorld == "dream":
            -> dragonplush_dream
            
        - currentWorld == "real" && dragonplush_dream:
            -> dragonplush_dreamvisited
        
        - else:
            -> dragonplush_real
    }
//---------------- DREAM WORLD ----------------

== dragonplush_dream ==

    //the dream dragon plush file
    //there's a massive Puff the Magic Dragon plush, twice as tall as the party. 
    //Nour's father used to read her the story in a sing-song voice; it was something she requested almost every night until she was in middle school.
    
    The faintest sound of music emanates from the bright green dragon plush. Something about the tune is familiar to Nour, but she doesn't recognize it, even as she hums along without realizing that she's doing.
    
    NOUR: ...Huh, that's weird. That sounds so familiar.
    
    NICK: What does?
    
    NOUR: You can't hear that music? It sounds like some kind of lullaby.
    
    NICK: Uhhmm... Nope, I've got nothing. You sure you're not hearing things, Nour?
    
    NOUR: Yeah, I'm sure. Whatever, let's keep moving.
    
    -> END


== dragonplush_realvisited ==

    //the dream dragon plush file
    //there's a massive Puff the Magic Dragon plush, twice as tall as the party. 
    //Nour's father used to read her the story in a sing-song voice; it was something she requested almost every night until she was in middle school.
    
    The sound of music emanates from the bright green dragon plush is a clear, lullaby-like tune. Nour runs her hand over the back of the dragon as she listens to it.
    
    NOUR: Oh, the plushy is here, too! He almost looks alive, and that music... It's the song..!!
    
    NICK: I've never heard that tune before! What is it?
    
    NOUR: It's a lullaby that tells the story of the children's book this dragon, Puff, is from. It's about a boy named Jackie and his imaginary-friend-dragon coming to life, in a way. At least that was my interpretation.
    
    NOUR: Now that I think about it, it feels strangely similar to this whole situation. Weird alternate dimension where our perception of things feels real to us. //feels too on the nose but for now
    
    NICK: That sounds like a stretch, but sure.
    
    NOUR: I miss how baba used to sing that for me every night. Kind of wish I could ask him to sing it to me again. I used to feel so safe whenever he did...
    
    NOUR: Weird how time flies, isn't it.
    
    NICK: Mmmmm, it goes pretty slow for me, to be honest.
    
    -> END


//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== dragonplush_real ==

    //the real dragon plush file
    //there's a normal-sized Puff the Magic Dragon plush sitting on a shelf.
    //Nour's father used to read her the story in a sing-song voice; it was something she requested almost every night until she was in middle school.
    
    //dream not visited
    
    NOUR: *Singing* Puff the magic dragon, lived by the sea...
    
    NOUR: Man, haven't thought about that song in years. I'm getting deja vu just from looking at that thing. //italicize all this so it's in her head
    
    NOUR: I didn't think it was anything special, but... Baba used to sing that song to me almost every night. I never stopped asking for it, not until I was in high school. 
    
    NOUR: It's hard to imagine that there was a time when I wasn't afraid to ask baba for things. That I didn't have to worry about what I said around him, or what he thought of me. 
    
    NOUR: I wanted him to be proud of me, but at the time it seemed like such a simple thing to do. 
    
    &NOUR: Now it feels impossible...
    
    -> END


== dragonplush_dreamvisited ==

    NOUR: Puff, go back to the alternate dimension, what are you doing here.
    
    NOUR: Stop following me around.
    
    -> END

//SUBSEQUENT REAL KNOTS HERE
