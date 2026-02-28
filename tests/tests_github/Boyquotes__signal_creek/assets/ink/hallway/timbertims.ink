== timbertims ==

    It's Timbertims, a store that exclusively sold bright white toe shoes with red soles.
    
    &FOLLOW_NICK_NickTimPos
    &FOLLOW_NOUR_NourTimPos
    &FOLLOW_SUWAN_SuwanTimPos
    
    &PAUSE_1.0
    
    &EMOTE_SUWAN_RightIdle
    &EMOTE_NICK_UpIdle
    &EMOTE_NOUR_UpIdle
    
    Despite its off-putting decor, everyone seems to have owned a pair of Timbertims at one point.

    {
        - timbertimsvisited:
            -> explored_everything
        - timbertims1:
            -> timbertimsvisited
        - else:
            -> timbertims1
            
    }
    
    &FOLLOW_NOUR_stop
    &FOLLOW_NICK_NOUR
    &FOLLOW_SUWAN_NICK
    
== timbertims1 ==

    &PORTRAIT_nick_nickpondering

    NICK: "Hey, did you guys ever get shoes from here before?"
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Yeah, actually... I had a pair back when I was in high school."
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "Wore them for like a week, and then... I dunno. Guess I realized how ugly they were."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "SAME! I had mine for a month before I had to give it to my younger brother."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "I recieved a pair as a gift once, though they aren't my style in the slightest."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "I suppose that speaks to how successful the marketing was."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Right... what was that slogan, again? That thing was a real earworm."

    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "<i>Once you Tim, you never Tout~</i>"
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "Yep. Makes even less sense than I remember. Catchy little tune, though."
    
    // SCREENSHAKE. TIM SPRITE SHLORPS IN. 
    
    &SHLORP_POOP_Tim_in
    
    &PAUSE_1.0
    
    // CAN WE GET ALL OF THE PLAYER CHARACTERS JUMPING IN SURPRISE OR TURNING AROUND TO FACE HIM?
    
    &EMOTE_NOUR_Jump
    &EMOTE_NICK_Jump
    &EMOTE_SUWAN_Jump
    
    &PAUSE_0.5 
    
    &EMOTE_SUWAN_RightIdle
    &EMOTE_NICK_UpIdle
    &EMOTE_NOUR_UpIdle
    
    &PORTRAIT_tim_timsmile
    
    TIM: "CUSTOMERS?! CUSTOMERS AT TIMBERTIMS?!"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "Whoa-!"
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "What the f-"
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "Can we help you?"
    
    TIM: "The question you should be asking is... can <i>I</i> help <i>you</i>?"
    
    &PORTRAIT_tim_timannoyed
    
    TIM: "Because I see none of you are wearing Tims on your little feetsies..."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "Our little... uhhh... ooookay..."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>Looks like this guy's the Timbertims manager.</i>
    
    NOUR: <i>But why is he even here if the store is closed...?</i>
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>Either way, I guess it might be worth it to speak to him.</i>


        + ["Excuse me, but do you know anything about how to get out of this mall?"] -> timbertimspol
        
        + ["Um, excuse me, but what are you even doing here?"] -> timbertimssus
    
    &FOLLOW_NOUR_stop
    &FOLLOW_NICK_NOUR
    &FOLLOW_SUWAN_NICK
    
    -> END

== timbertimspol ==

    NOUR: "Excuse me, but do you know anything about how to get out of this mall?"
    
    &PORTRAIT_tim_timsmile

    TIM: "Why, yes! I do."
    
    &PORTRAIT_tim_timsmile
    
    TIM: "Wear some Timbertims, and you may go as you please~"
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "... I don't exactly have time for new shoes right now."
    
    NOUR: "I just need to know how to leave."
    
    &PORTRAIT_tim_timneutral
    
    TIM: "Leaving is a good idea. I'm planning to leave soon, myself."
    
    TIM: "Move to some new town, settle down..."
    
    &PORTRAIT_tim_timsmile
    
    TIM: "Meet the love of my life, raise a family of timlings..."
    
    &PORTRAIT_tim_timnervous
    
    TIM: "My life is both successful yet lonely..."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Um - okay, I'm happy for you, but that doesn't help us at all."
    
    &PORTRAIT_tim_timneutral
    
    TIM: "Maybe open a <i>Timbertims</i> there too and sell them en masse..." 
    
    TIM: "Or maybe I should leave my small business days behind..."
    
    TIM: "I've always wanted to be a politician."
    
    &PORTRAIT_tim_timsmile
    
    TIM: "Or a god!"
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "Are you even listening to me anymore?"
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "I believe this man is under the influence. We should leave him alone."
    
    TIM: "Under the Timfluence..."
    
    &SHLORP_POOP_Tim_out
    
    &FOLLOW_NOUR_stop
    
    NOUR: "Interesting."
    
    &FOLLOW_NOUR_stop
    &FOLLOW_NICK_NOUR
    &FOLLOW_SUWAN_NICK
    
        -> END

== timbertimssus ==
    
    NOUR: "Hasn't your store been closed down for weeks?"
    
    &PORTRAIT_tim_timannoyed
    
    TIM: "As long as somebody in Signal Creek is wearing a pair of <i>Timbertims</i>..."
    
    TIM: "I can stay as long as I want."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "Uh... okay. Whatever that means."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "I was under the impression the <i>Timbertims</i> fad died out months ago."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "I've hardly noticed anyone with this brand at all recently."

    &PORTRAIT_nick_nickneutral
    
    NICK: "Oh... actually, my brother still wears <i>Timbertims</i>." 

    &PORTRAIT_nick_nicksad
    
    NICK: "We, um... we don't really get new shoes often." 
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "I mean - my mom's been planning on buying us new ones soon. 
    
    NICK: "She hasn't had the time yet to take us shopping."
    
    &PORTRAIT_tim_timneutral
    
    TIM: "Nick's brother's little feetsies are the last sources of power..."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "Can you please stop saying 'little feetsies'?"
    
    &PORTRAIT_tim_timannoyed
    
    TIM: "But if my savior exchanged his Tims for some inferior shoes..."
    
    &PORTRAIT_tim_timnervous
    
    TIM: "I fear my time in Signal Creek is limited..."
    
    MS. SUWAN: "On that topic, have you ever found a way to leave this mall?"
    
    &PORTRAIT_tim_timneutral
    
    TIM: "A pair of <i>Timbertims</i> may show you the path to salvation!"
    
    TIM: "Salvation being an escape from this mall...?"
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "... I don't exactly have time for new shoes right now. I just need to know how to leave."
    
    TIM: "Leaving is a good idea. I'm planning to leave soon, myself."
    
    TIM: "Move to some new town, settle down..."
    
    TIM: "Meet the love of my life, raise a family of timlings..."
    
    TIM: "My life is both successful yet lonely..."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "Um - okay, I'm happy for you, but that doesn't help us at all."
    
    TIM: "Maybe open a <i>Timbertims</i> there too and sell them en masse..." 
    
    TIM: "Or maybe I should leave my small business days behind..."
    
    &PORTRAIT_tim_timsmile
    
    TIM: "I've always wanted to be a politician."
    
    TIM: "Or a god!"
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "Well, thanks for nothing, I guess."
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>Something tells me he isn't going to help us at all.</i>
    
    NOUR: <i>Might as well leave him alone.</i>

    &SHLORP_POOP_Tim_out
    
    &FOLLOW_NOUR_stop
    &FOLLOW_NICK_NOUR
    &FOLLOW_SUWAN_NICK
    
    NOUR: "Interesting."

    -> END


== timbertimsvisited ==

    &PORTRAIT_nick_nickpondering

    NICK: "Isn't it weird how <i>everyone</i> was wearing Tims for like a week?"
    
    NICK: "Like, did anyone else notice that?"
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "I sure did."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "I suppose that's how fads work."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "But I agree—it was strange."
    
    MS. SUWAN: "Especially considering how unpleasant those shoes are..."
    
    &SHLORP_POOP_Tim_in
    
    TIM: "UNPLEASANT?"

    &PORTRAIT_nick_nicknervous
    
    NICK: "Uh, she meant - <i>totally</i> pleasant!"
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "I certainly did not."
    
    &SHLORP_POOP_Tim_out
    
    &FOLLOW_NOUR_stop
    &FOLLOW_NICK_NOUR
    &FOLLOW_SUWAN_NICK
    
    NOUR: "There he goes, again."

    -> END
