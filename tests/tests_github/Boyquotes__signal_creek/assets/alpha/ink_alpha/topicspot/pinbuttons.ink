== pinbuttons ==

    {
        - currentWorld == "dream" && pinbuttons_real:
            -> pinbuttons_realvisited
            
        - currentWorld == "dream":
            -> pinbuttons_dream
            
        - currentWorld == "real" && pinbuttons_dream:
            -> pinbuttons_dreamvisited
        
        - else:
            -> pinbuttons_real
    }
//---------------- DREAM WORLD ----------------

== pinbuttons_dream ==

    It's a display of buttons and pins. There's a hammer and sickle pin, a button depicting the cross over an American flag, and various other obscenities that Ms. Suwan doesn't even want to acknowledge.
    
    MS SUWAN: God, why are they selling these things?
    
    -> politicalConvo


== pinbuttons_realvisited ==

    It's a display of buttons and pins. There's a hammer and sickle pin, a button depicting the cross over an American flag, and various other obscenities that Ms. Suwan doesn't even want to acknowledge.
    
    MS SUWAN: This is, uh, a lot more extreme than the friendly political messages I saw before.
    
    -> politicalConvo

    
== politicalConvo ==

    Nour and Ms Suwan discuss extreme political ideologies.
    
    -> END

//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== pinbuttons_real ==

    It's a display of buttons and pins. There's a pride flag pin, a button adorning an anarchy symbol, and others with messages like "Eat. Sleep. Game. Repeat."
    
    NICK: Oh, sick! A gamer pin!
    
    Nick picks the pin up for a moment, looks over it contemplatively, and returns it to the display.
    
    -> END


== pinbuttons_dreamvisited ==

    It's a display of buttons and pins. There's a pride flag pin, a button adorning an anarchy symbol, and others with messages like "Eat. Sleep. Game. Repeat."
    
    MS SUWAN: Ah. Back to the typical corporate-friendly political statements.
    
    -> END

//SUBSEQUENT REAL KNOTS HERE
