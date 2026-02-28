== womensclothing ==

    {
        - currentWorld == "dream" && womensclothing_real:
            -> womensclothing_realvisited
            
        - currentWorld == "dream":
            -> womensclothing_dream
            
        - currentWorld == "real" && womensclothing_dream:
            -> womensclothing_dreamvisited
        
        - else:
            -> womensclothing_real
    }
//---------------- DREAM WORLD ----------------

== womensclothing_dream ==

+   { victorias_dream } -> cbk_appears

+   -> womensclothing_normal

= cbk_appears
    
    //here, pass a variable to godot to call a function that makes cbk appear in the hallway
    -> womensclothing_normal
    
= womensclothing_normal

    It's a women's clothing store, flooded with shoppers. Vibrant posters of women wearing designer clothing are plastered on the windows.

    &MS SUWAN: Oh, I remember this store.

    -> END
    
= womensclothing_dream_sailormoon
    
    NOUR: Now's my chance to live out my dreams of being like Sailor Uranus... I see tons of androgynous clothing in there!
    
    -> END

== womensclothing_realvisited ==

+   { victorias_dream } -> cbk_appears_realvisited

+   -> womensclothing_realvisited_normal

= cbk_appears_realvisited
    //here, pass a variable to godot to call a function that makes cbk appear in the hallway
    -> womensclothing_realvisited_normal

= womensclothing_realvisited_normal

    This is how the women's clothing store was before: flooded with shoppers. Vibrant posters of women wearing designer clothing are plastered on the windows. The mannequins inside are wearing the latest trends.
    
    MS SUWAN: I knew those dusty mannequins looked familiar...
    
    &MS SUWAN: I remember this store.
    
    
    
    -> END

//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== womensclothing_real ==

=womensclothing_real_normal
    
    This store looks like it used to be a women's clothing store. There's torn fliers on the glass, and signage exclaiming "EVERYTHING MUST GO!!!!"
    
    There's mannequins strewn about, and lines of empty shelving.
    
    MS SUWAN: Looks like this store was popular a long time ago.
    
    { sailormoonposters_dream } -> womensclothing_real_sailormoon
    
    -> END

=womensclothing_real_sailormoon

    NOUR: Ah, I wish there could be gender neutral clothing...

    -> END


== womensclothing_dreamvisited ==

    That clothing store is closed now. There's torn fliers on the glass, and signage exclaiming "EVERYTHING MUST GO!!!!"
    
    There's mannequins strewn about, and lines of empty shelving.
    
    MS SUWAN: Those obnoxious people are gone. Thank god.
    
    { sailormoonposters_dream } -> womensclothing_dreamvisited_sailormoon
    
    -> END
    
=womensclothing_dreamvisited_sailormoon

    NOUR: Ah, I wish there could be gender neutral clothing...

    -> END

//SUBSEQUENT REAL KNOTS HERE
