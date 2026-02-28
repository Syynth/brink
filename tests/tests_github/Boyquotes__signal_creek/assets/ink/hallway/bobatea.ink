=== bobatea ===
    
    The vending machine is full of coffee, sparkling water, and diet sodas.
    
    The last row in the vending machine is a row of Specialty Drinks— Japanese sodas and canned boba. 
    
    {
        - bobatea_find_dukedelicious:
            -> explored_everything
            
        - consoles_start_aplequest:
            -> bobatea_find_dukedelicious
            
        - bobatea_beforeaplequest:
            -> bobatea_beforeaplequest_repeat
            
        - else:
            -> bobatea_beforeaplequest
    }
    
    -> END

=== bobatea_beforeaplequest ===

    Nick's nose scrunches up.
    
    &FOLLOW_NICK_NickBobaStart
    
    &FOLLOW_SUWAN_SuwanBobaStart
    
    &FOLLOW_NOUR_NourBobaStart
    
    &PAUSE_1.0
    
    &EMOTE_NICK_UpIdle
    
    &EMOTE_SUWAN_UpIdle
    
    &EMOTE_NOUR_UpIdle
    
    &PORTRAIT_nick_nickangry
    
    NICK: "Bleh! Most of these drinks suck."
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "These <i>are</i> more like liquid sugar than caffeine... and the bad kind of liquid sugar, too."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "I don't recognize any of these brands. I find that disturbing."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "These are all gross except for the boba!"
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Wouldn't you rather get fresh boba from an actual shop?"
    
    NICK: "Mmm... well..."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "The canned stuff is cheaper so..."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "Oh... yeah, that's fair." 
    
    &PORTRAIT_nour_nourneutral
    
        + ["Do you want me to get you one?"]
        
            &PORTRAIT_nick_nickneutral
        
            NICK: "No thanks. I'm ok."
            
            &PORTRAIT_nour_nourneutral
            
            NOUR: "Are you sure?"
            
            NICK: "Yeah. If I accept something from someone, I'd have to repay them."
        
        + ["If no one's gonna get anything, let's go."]
    
    - NOUR: "We should move on to something else."
    
        -> END

=== bobatea_beforeaplequest_repeat ===

    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>We've already looked at all the drinks.</i>
        
        -> END

=== bobatea_find_dukedelicious ===

    &PAUSE_1.0
    
    &AUDIO_FADEOUT
    
    &PORTRAIT_nour_nourrealization

    NOUR: <i>...Wait a minute. What's up with that boba can in the back?</i>
    
    &FOLLOW_NICK_NickBobaStart
    
    &FOLLOW_SUWAN_SuwanBobaStart
    
    &FOLLOW_NOUR_NourBobaStart
    
    &PAUSE_1.0
    
    &EMOTE_NICK_UpIdle
    
    &EMOTE_SUWAN_UpIdle
    
    &EMOTE_NOUR_UpIdle
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "I think there's something... <i>wriggling</i> inside that can?"
    
    &PORTRAIT_nour_nourrealization

    NOUR: "It looks kinda round... like an apple?!"
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "We should buy the boba to free it. Do you want the boba, Nick?"
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "I'm sure it'll taste real, um, apple-y. That's not too bad, right?"
    
    &PORTRAIT_nick_nickneutral

    NICK: "..."
    
    Nick shrugs. 
    
    After a moment, Ms. Suwan clears her throat.
    
    &PORTRAIT_ms. suwan_mssuwanneutral
        
    MS. SUWAN: "It's been quite an eventful day so far. Perhaps a drink is warranted."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "It'd be a waste to get rid of the boba for the sake of an apple."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "..."
    
    Nick shakes his head 'no'.
    
    &PORTRAIT_nour_nourneutral
    
        * ["We could all pitch in."]
        
        * ["Should we try to break it open?"]
            
            &PORTRAIT_nour_nourpondering
            
            NOUR: "It could be time for a little B&E on this machine. What do you say?"
            
            &PORTRAIT_nour_noursmile
            
            NOUR: "It would be free."
            
            &PORTRAIT_ms. suwan_mssuwanannoyed
            
            MS. SUWAN: "Let's be reasonable, please. We could simply all pitch in and pay."
            
            &PORTRAIT_nick_nicksmallsmile
            
            NICK: "..."
            
            &PORTRAIT_nour_noursmile
            
            NOUR: <i>Made him smile. Nice!</i>
        
    - &PORTRAIT_nour_nourneutral 
    
    NOUR: "Would you be more comfortable if we all split the price of the drink?"
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "...50 cents each?"
    
    &PORTRAIT_ms. suwan_mssuwanneutral
        
    MS. SUWAN: "Exactly."
    
    &PORTRAIT_nick_nickneutral
    
    NICK: "...You have some, too. Make it fair."
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "We will."
    
    &FOLLOW_NICK_BobaQuarter
    
    // SFX COINS INTO VENDING MACHINE
    
    &PAUSE_0.5
    
    &SOUND_insertcoin
    
    Nick fishes out two quarters from his pocket and inserts it into the vending machine.
    
    // SFX CAN DISPENSED FROM MACHINE
    
    &SOUND_sodacandrop
    
    More quarters enter the vending machine. Soon, a can of boba rolls out.
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "Here it is."
    
        * "I'll crack it open."
        
        * "Ms. Suwan, could you open this?"
            
            &PORTRAIT_nour_noursmile
            
            NOUR: "I trust you the most."
            
            &PORTRAIT_ms. suwan_mssuwansmile
            
            MS. SUWAN: "Very well."
            
            &FOLLOW_SUWAN_RetrieveBoba
            
            &PAUSE_0.5
            
            &EMOTE_SUWAN_UpIdle
        
        * "Do you wanna open this, Nick?"
        
            &PORTRAIT_nour_noursmile
        
            NOUR: "You know more about all of this than the two of us."
            
            &PORTRAIT_ms. suwan_mssuwansmile
            
            MS. SUWAN: "Agreed."
            
            &PORTRAIT_nick_nickneutral
            
            NICK: "..."
            
            Nick shrugs again and takes the can.
            
            &FOLLOW_NICK_RetrieveBoba
            
            &PAUSE_0.5
            
            &EMOTE_NICK_UpIdle
    
    - The can is opened...
    
    //&SOUND_
    
    &FOLLOW_NICK_NickBobaStart
    
    &FOLLOW_SUWAN_SuwanBobaStart
    
    &FOLLOW_NOUR_NourBobaStart
    
    &PAUSE_1.0
    
    &EMOTE_NICK_UpIdle
    &EMOTE_SUWAN_UpIdle
    &EMOTE_NOUR_UpIdle
    
    ...and an apple squeezes out!
    
    &SHLORP_APPLE_DukeDelicious_in
    
    &PORTRAIT_???_dukedeliciousneutral
    
    &SOUND_dukedeliciouslaugh
    
    ???: "FREEDOM..."
    
    &EMOTE_NOUR_Jump
    &EMOTE_NICK_Jump
    &EMOTE_SUWAN_Jump
    
    &PAUSE_0.5 
    
    &EMOTE_NICK_UpIdle
    &EMOTE_SUWAN_UpIdle
    &EMOTE_NOUR_UpIdle
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "...!"
    
    &PORTRAIT_???_dukedeliciouspreach
    
    ???: "SWEET, SWEET FREEDOM!"
    
    &PAUSE_1.0

    &MUSIC_ApleQuestMusic
    
    &PORTRAIT_???_dukedelicioussmile
    
    ???: "THIS CYLINDRICAL METAL PRISON SHALL NO LONGER CONTAIN ME!"
    
    &PORTRAIT_nour_nourrealization
        
    NOUR: "Um, hello. We're here on behalf of Emperor Evercrisp to rescue you."
    
    &SOUND_dukedeliciouslaugh
    
    ???: "Of <i>course</i> the Emperor has sent you fine heroes to free me! Thank you! HAR HAR!"
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "What's your name?"
    
    &PORTRAIT_duke delicious_dukedeliciouspreach
    
    DUKE DELICIOUS: "It is I, <i>DUKE DELICIOUS</i>!"
    
    &PORTRAIT_duke delicious_dukedelicioussmile
    
    DUKE DELICIOUS: "But my friends call me Red."
    
    &PORTRAIT_duke delicious_dukedelicioussad
    
    DUKE DELICIOUS: "Alas, I have failed to find my prince."
    
    &PORTRAIT_duke delicious_dukedelicioussmile
    
    DUKE DELICIOUS: "I hope that you may continue in my stead and bear the fruits of success, HAR HAR!"
    
    &PORTRAIT_duke delicious_dukedeliciouspreach
    
    DUKE DELICIOUS: "As a small reward, let me tell you something special."
    
    &PORTRAIT_duke delicious_dukedelicioussmile
    
    DUKE DELICIOUS: "Something that may help you on your multi-plane journey, Plane Walkers!"
    
    &PORTRAIT_duke delicious_dukedeliciouspreach
    
    DUKE DELICIOUS: "<i>Remember we are ALL children of the Mother Tree...</i>"
    
    &PORTRAIT_duke delicious_dukedelicioussmile
    
    &SOUND_dukedeliciouslaugh
    
    DUKE DELICIOUS: "<i>So show MERCY to the BERRY BEAST!</i>
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Huh. Interesting..."
    
    //questions here from the party to the Duke?
    
    &PORTRAIT_duke delicious_dukedeliciousneutral
    
    DUKE DELICIOUS: "Now that I have imparted my wisdom."
    
    &PORTRAIT_duke delicious_dukedelicioussmile
    
    DUKE DELICIOUS: "It is time for me to rejoin my liege, but humans!"
    
    &PORTRAIT_duke delicious_dukedeliciouspreach
    
    DUKE DELICIOUS: "Charge forth and SEARCH FOR THE PRINCE!"
    
    &SHLORP_APPLE_DukeDelicious_out
    
    &PAUSE_0.5
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "Well, you heard what the Duke said. Shall we?"
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "Very well.
    
    &PORTRAIT_nick_nickneutral
    
    NICK: "..."
    
    Nick drinks the canned boba. 
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "How's that tasting, Nick?"
    
    &PORTRAIT_nick_nickneutral
    
    NICK: "...Bad."
    
    Nick gulps more of it down.
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "...Sure it is. Make sure to save some for the rest of us."
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "..."
    
    &MUSIC_STOP
    
    NOUR: <i>Looks like he's feeling a little better.</i>
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>Now time to find some more apples...</i>
    
    &PAUSE_1.0
    
    &MUSIC_HallwayMusic
    
    &FOLLOW_NOUR_stop
    
    -> END