== pretzelstand ==

    {
        - currentWorld == "dream" && pretzelstand_real:
            -> pretzelstand_realvisited
            
        - currentWorld == "dream":
            -> pretzelstand_dream
            
        - currentWorld == "real" && pretzelstand_dream:
            -> pretzelstand_dreamvisited
        
        - else:
            -> pretzelstand_real
    }
//---------------- DREAM WORLD ----------------

== pretzelstand_dream ==

    NOUR: Man, this looks like a great place to try some cinnamon sugar bites! I was never allowed to get junk food as a kid...
    
    :NOUR:
    
    + [Talk to the guy]NOUR: Uh... Hello? Do you guys have cinnamon sugar bites?
        
        -> noresponse
    
= noresponse

    No response.
    
    :NOUR:
    + [Huh? Why the hell isn't he responding?]NOUR: Hello? Hey, are you deaf or something?

        -> stillnoresponse

= stillnoresponse

    Still, no response.

    -> END


== pretzelstand_realvisited ==

    NOUR: Looks like now's my chance to try some junk food!
    
    :NOUR:
        + [Talk to the guy]NOUR: Uh... Hello? Do you guys have cinnamon sugar bites?
            
            -> noresponse
    
    = noresponse
    
        No response.
        
        :NOUR:
        
        + [Huh? Why the hell isn't he responding?]NOUR: Hello? Hey, are you deaf or something?
            
            -> stillnoresponse
        
    = stillnoresponse
    
        Still, no response.
    
        -> END


//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== pretzelstand_real ==

    //CHECK IF THIS KNOT WAS VISITED, WHEN WE'RE IN: rina.ink
    //Nour and Nick will bond over not being able to buy something, recalling this conversation.
    
    NOUR: This is the place I'd stop to get snacks with my mom, back when I was a kid.
    
    NOUR: I wasn't really allowed to have junk food, so it was exciting to get a pretzel
    
    NOUR: I wish I could've gotten cinnamon sugar bites.
    
    NICK: Those doughnut holes filled with sugary goo were so good... I got them one time, and ate them all in like, two minutes!
    
    MS SUWAN: I may have treated myself to a pretzel, once or twice.
    
    NICK: That's so boring! You didn't even need your parent's permission to get something fun and sugary, and you still chose pretzels?
    
    NOUR: I guess pretzels are fine, but god, it was so frustrating that I didn't really get a choice.

    -> END

== pretzelstand_dreamvisited ==

    *   -> pretzelstand_real

    -> END

//SUBSEQUENT REAL KNOTS HERE


