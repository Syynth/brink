    == partyposter ==

    {
        - partyposter2:
            -> partypostervisited
        - else:
            -> partyposter1

    }
    
== partypostervisited ==
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "Party hasn't happened yet, we could still go..."
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "No, Nick."
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "No, Nick."
    
    'NO, NICK is now displayed in fun bubbly font on the poster!'
    
    -> END
    
    
== partyposter1 ==
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "Oh, wow. Are these posters for a party or something?"
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "Whoa, free food and drinks...! Hey, we should all go together!"
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "I think going to a party should be the last of our priorities right now."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "I'd rather spend as little time in this mall as possible."
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "Well... okay, you're right, but --"
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "No, Nick's right."
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "We should totally go to this super normal party in this super normal mall."
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "That... was sarcasm... right...?"
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "Yep."
    
    &PORTRAIT_nick_nickangry
    
    NICK: "I'm just saying! It could be fun! I mean, you can never go wrong with free snacks."
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "You have no idea what could be in the food here."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "We can't rule out that all of this is caused by a shared drug hallucination just yet."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "Besides, we don't have much reason to socialize with anyone in this place."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "... Unless they're able to help us leave."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "That seems kinda cold."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "Hold on, there's something weird about this thing..."
    
    + [ Take a closer look. ] -> partyposter2
    + [ Leave the poster alone. ] -> partyposterend 
    

== partyposter2 ==
    
    Underneath the basic date, time, and location of the party, there's more words.
    
    They're written in fun bubbly font.
    
    Oddly, they seem to ripple and change...
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "What the hell...?"
    
    'WHAT THE HELL' is now emblazoned on the poster!
    
    &PORTRAIT_ms. suwan_suwannervous
    
    MS. SUWAN: "It seems to be displaying whatever was last said. This is... extremely strange."
    
    'THIS IS EXTREMELY STRANGE' is now emblazoned in fun bubbly font on the poster!
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "Whoa, that is so freaky! Wait, I wonder if I can..."
    
    &PORTRAIT_nour_nourangry
    
    NOUR: "Don't you try it."
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "FUUUUCK!"
    
    'F@&%?$! is now displayed on the poster!'
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "<i>Sigh.</i> I knew it."
    
    &PORTRAIT_nick_nickangry
    
    NICK: "You gotta be kidding me. This mall is rated PG?!"

    -> END
    
== partyposterend ==
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "There's probably nothing useful on it, anyway."
    
    -> END
