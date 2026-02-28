== victorias ==

    {
        - currentWorld == "dream" && victorias_real:
            -> victorias_realvisited
            
        - currentWorld == "dream":
            -> victorias_dream
            
        - currentWorld == "real" && victorias_dream:
            -> victorias_dreamvisited
        
        - else:
            -> victorias_real
    }
//---------------- DREAM WORLD ----------------

== victorias_dream ==

//Nour filled the classic young-closeted-gay child who got very flustered whenever she went near or in a victoria's secret

    NOUR: Ah. Yes. The store that caused me endless embarrassment as a child.
    
    MS SUWAN: Oh? How come?
    
    NOUR: ...Uhh. Because.
    
    MS SUWAN: Because..?
    
    NOUR: ...Beautiful women. I will explain no further.
    
    MS SUWAN: Haha, I think I understand.
    
    MS SUWAN: Femininity is so strange, isn't it? When it's commodified and sold to us in a very specific form, particularly.
    
    NOUR: I mean, yeah. I used to think it's all part of this weird, lace-covered box society is trying to push women into. 
    
    NOUR: But I realized it can make a lot of people feel really powerful, which shouldn't be forgotten, y'know?
    
    NOUR: ...Not me, though. I was pushed into being feminine a little too much to enjoy it.
    
    MS SUWAN: All very well and fair. 
    
    MS SUWAN: I leaned into femininity very strongly when I was younger... To put it plainly, it was the least unbearable way for me to exist as a woman. At least, that's how I felt at the time. 
    
    NICK: I don't know much about being feminine but it always felt weird to walk by that store. Kinda makes me uncomfortable.
    
    -> END

== victorias_realvisited ==

//Nour filled the classic young-closeted-gay child who got very flustered whenever she went near or in a victoria's secret

    NOUR: Shit, seems like it's up-and-running here. Aaaand all the lingerie and posters are back, lovely.
    
    NOUR: ...Heh. Looking back at how I used to react to this place when I was a kid, I'm not sure how I and everyone around me missed the clear signs that I was gay.
    
    NICK: Maybe they *didn't.*
    
    NOUR: ...What.
    
    NICK: Maybe they *did* know but didn't tell you!! Haha, especially with how red you're getting.
    
    NOUR: Shut up, kid!
    
    -> END

//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== victorias_real ==

//Nour filled the classic young-closeted-gay child who got very flustered whenever she went near or in a victoria's secret

    NOUR: Don't look don't look don't look don't look don--
    
    NOUR: ...Oh. The windows are empty. Guess they cleared out all the stock and the posters and everything. 
    
    -> END

== victorias_dreamvisited ==

    NOUR: Jeez, can't seem to escape this store.
    
    NOUR: Their clothes aren't even comfortable! Purely sex appeal.
    
    NICK: Is it workiiiiiing~?
    
    NOUR: ...I plead the fifth.
    
    -> END

//SUBSEQUENT REAL KNOTS HERE
