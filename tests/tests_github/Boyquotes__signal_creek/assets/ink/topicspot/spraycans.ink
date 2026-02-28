== spraycans ==

    It's a bunch of spray cans. 
    
    {
        - spraycans_afternickroute:
            -> explored_everything
            
        - nourplaysaplequest:
            -> spraycans_afternickroute
            
        - spraycans_start:
            -> spraycans_start_repeat
            
        - consoles:
            -> spraycans_start
    }
    
    -> END

== spraycans_start ==

    &FOLLOW_NICK_NickCheckOut
    
    &FOLLOW_NOUR_NourCheckOut
    
    &FOLLOW_SUWAN_SuwanCheckOut
    
    &PAUSE_1.0
    
    &EMOTE_NICK_RightIdle
    
    &EMOTE_NOUR_RightIdle
    
    &EMOTE_SUWAN_RightIdle
    
    &EMOTE_NICK_Jump
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "Ooh! Hey, guys! Think you could buy me one of these?"
    
    NICK: "I'll pay you back later." 
    
        &PORTRAIT_nour_nourpondering
    
    NOUR: "...I didn't know you liked graffitti art."
    
        &PORTRAIT_nick_nicknervous
        
    &EMOTE_NICK_LeftIdle
    
    NICK: "Well, I don't really know how to do it..."
    
    &PORTRAIT_nour_nourannoyed

    NOUR: "Then... what are you going to do with these?"
    
        &PORTRAIT_nick_nicknervous
        
    &EMOTE_NICK_RightIdle
    
    NICK: "It's... it's <i>none</i> of your business. Just lay off."
    
        &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "You want to huff it, don't you."
    
        &PORTRAIT_nour_nourannoyed
        
    &EMOTE_NOUR_DownIdle
    
    NOUR: "Oh, no way. He wouldn't."
    
        &PORTRAIT_ms. suwan_mssuwanangry
        
    &EMOTE_SUWAN_UpIdle
    
    MS. SUWAN: "There are <i>many</i> reasons why entrusting a child with these is a terrible idea."
    
        &PORTRAIT_nick_nickangry
    
    NICK: "Who <i>cares</i> about that? I just want to have a little fun, geez!"
    
        &PORTRAIT_nour_nourshocked
        
    &EMOTE_NOUR_RightIdle
    
    &EMOTE_SUWAN_RightIdle
    
    NOUR: "NICK!"
    
        &PORTRAIT_nick_nicksad
        
    &EMOTE_NICK_LeftIdle
    
    &EMOTE_NICK_Jump
    
    NICK: "<i>Please</i>...? I'll pay you back afterwards, promise!"
    
        &PORTRAIT_nour_nourannoyed
    
    NOUR: "I'm going to have to agree with Ms. Suwan here."
    
    NOUR: "There's a reason why you have to be over 18 to purchase these."
    
        &PORTRAIT_ms. suwan_mssuwanannoyed

    MS. SUWAN: "Nick, getting high is not a good path to take."
    
        &PORTRAIT_nick_nickangry
        
    &EMOTE_NICK_LeftIdle
    
    NICK: "Sheesh, you sound like those <i>Say No To Drugs</i> assemblies we have in school."
    
        &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "Good. That means I'm saying what you need to hear."
    
        &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "Substance abuse is addictive, and will only harm your future."
    
        &PORTRAIT_nick_nicksad
    
    NICK: "But I have friends who do stuff like this all of the time, and they're fine!"
    
        &PORTRAIT_nick_nickshadow

    NICK: "It's not like I have a bright future, anyways. Might as well have some fun."
    
        &PORTRAIT_nour_noursad
    
    NOUR: "Nick, there's plenty of other things you can do for fun."
    
        &PORTRAIT_nour_nourpondering
    
    NOUR: "Things that <i>aren't</i> dangerous that you'll actually enjoy."
    
        &PORTRAIT_nick_nickangry
    
    NICK: "Oh yeah? Like <i>what?</i>"
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "Alright, give me a bit, and we can definitely find something fun to do in this mall."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>Maybe I can introduce him to photography?</i>
    
    NOUR: <i>That, or reading. I enjoy both, so maybe he will too...</i>
    
    &PORTRAIT_nick_nicksad
    
    NICK: "I hope that whatever you're planning to show me is <i>SO</i> much fun!"
    
    NOUR: <i>I should lead Nick somewhere that would inspire new interests.</i>
    
    &FOLLOW_NOUR_stop
    
    &FOLLOW_NICK_NOUR
    
    &FOLLOW_SUWAN_NICK
    
    &LIGHT_Nick0
    
    -> light_on
// -> END

== spraycans_start_repeat ==

    &FOLLOW_NOUR_NourCheckOut
    
    &FOLLOW_SUWAN_SuwanCheckOut
    
    &FOLLOW_NICK_NickCheckOut
    
    &PAUSE_1.0
    
    &EMOTE_NOUR_RightIdle
    
    &EMOTE_SUWAN_RightIdle
    
    &EMOTE_NICK_RightIdle

    &PORTRAIT_nick_nicksad

    NICK: "Bringing me back here isn't really helping your point about 'better hobbies' or whatever."
    
        &PORTRAIT_nour_nourannoyed
    
    NOUR: "Okay, smart guy. I'll find something to prove my point."
    
        &PORTRAIT_nour_nourpondering
    
    NOUR: <i>Earlier, Nick said something about how he doesn't feel like he has a bright future.</i>
    
    NOUR: <i>It sounds like he doesn't have much confidence in his prospects.</i>
    
    NOUR: <i>I should help him find a hobby. Something that would give him a purpose.</i>
    
        &PORTRAIT_nour_noursmile
    
    NOUR: <i>Maybe we could go somewhere with books and read up about new things.</i>
    
    &FOLLOW_NOUR_stop
    
    &FOLLOW_NICK_NOUR
    
    &FOLLOW_SUWAN_NICK
    
    -> END

== spraycans_afternickroute ==

    &FOLLOW_NOUR_NourCheckOut
    
    &FOLLOW_SUWAN_SuwanCheckOut
    
    &FOLLOW_NICK_NickCheckOut
    
    &PAUSE_1.0
    
    &EMOTE_NOUR_RightIdle
    
    &EMOTE_SUWAN_RightIdle
    
    &EMOTE_NICK_RightIdle

        &PORTRAIT_nour_nourneutral

    NOUR: "When you're an adult, you can purchase whatever you want."
    
        &PORTRAIT_nour_noursmile
    
    NOUR: "We just hope they're decisions that benefit you, not harm you."
    
        &PORTRAIT_ms. suwan_mssuwansmile

    MS. SUWAN: "Precisely."
    
        &PORTRAIT_nick_nicksmallsmile
        
    &EMOTE_NICK_LeftIdle

    NICK: "Don't worry. These didn't <i>really</i> interest me in the first place."
    
        &PORTRAIT_nick_nicksad
    
    NICK: "I just thought 'cause my friends did it... I should, too." 
    
    NICK: "Or... something."
    
        &PORTRAIT_nick_nicksmallsmile
    
    NICK: "But I'd much rather play Aple Quest..."
    
    NICK: "So no more lectures, alright? I learned my lesson!"
    
    NOUR: "Okay, okay. Good to know, Nick."
    
    NICK: "And since you both taught me this <i>gracious</i> lesson..."
    
        &PORTRAIT_nick_nickbigsmile
    
    NICK: "It's only right that I convert you both into <i>Aple Quest</i> fans too!"
    
        &PORTRAIT_ms. suwan_mssuwanrealization
    
    MS. SUWAN: "...Perhaps we did <i>too</i> good of a job."
    
        &PORTRAIT_nour_nourshocked
    
    NOUR: "What monster have we created..."
    
    NICK: "Haha, I can't wait!"
    
    &FOLLOW_NOUR_stop
    
    &FOLLOW_NICK_NOUR
    
    &FOLLOW_SUWAN_NICK
    
        -> END

