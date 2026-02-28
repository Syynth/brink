 === weirdtree ===

    It's a really weird tree. 
    
    It's extremely clear that this tree is not like the others. 
    
    {
        - weirdtree_findgranny:
            -> explored_everything
            
        - consoles_start_aplequest:
            -> weirdtree_findgranny
            
        - weirdtree_before_aplequest:
            -> weirdtree_before_aplequest_repeat
            
        - else:
            -> weirdtree_before_aplequest
    }
    
=== weirdtree_before_aplequest ===

    &PAUSE_1.0
    
    &AUDIO_FADEOUT

    &PORTRAIT_nour_nourpondering

    NOUR: "This tree looks strange..."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "Do you think there's something <i>in</i> it?"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "Dunno, but it looks WACK! It's all pink and wobbly..."
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "What if I just-"
    
    //FOLLOW_NICK_NickTreePos
    
    &PAUSE_1.0
    
    &PORTRAIT_ms. suwan_suwanannoyed
    
    MS. SUWAN: "Do <i>not</i> climb the tree."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "But what if there's something super duper cool up there?"
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "No. We have better things to do."
    
    &PORTRAIT_nick_nickangry
    
    NICK: "Ugh, fine. You're no fun."
    
    -> END

=== weirdtree_before_aplequest_repeat ===

    &PORTRAIT_ms. suwan_suwanannoyed
    
    MS. SUWAN: "Don't even think about it."
    
    &PORTRAIT_nick_nickshocked

    NICK: "..."
    
    &PORTRAIT_nick_nickangry
    
    NICK: "..."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>Maybe I can come back to this later...</i>
    
    -> END

=== weirdtree_findgranny ===

    &FOLLOW_NOUR_NourTreeStartPos
    
    &FOLLOW_SUWAN_SuwanTreeStartPos
    
    &FOLLOW_NICK_NickTreeStartPos

    &PORTRAIT_nour_noursmile
    
    NOUR: "... You know what I'm thinking?"
    
    &PAUSE_0.05
    
    &EMOTE_NOUR_UpIdle
    
    &EMOTE_NICK_RightIdle
    
    &EMOTE_SUWAN_UpIdle
    
    &PORTRAIT_ms. suwan_suwanannoyed

    MS. SUWAN: "An apple... in a tree...?"
    
    &PORTRAIT_nick_nickshadow
    
    NICK: "..."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "We could try cutting down the tree."
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "With what tools? This is a joke, right?"
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "No. I'm serious about this. We should go find an axe."
    
    &PORTRAIT_nick_nicknervous //he should look confused
    
    NICK: "..."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Should we try something else?"
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "..."
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "To check for an apple up in the tree, we could..."
    
        * (shake) "Shake the tree?"
        
        * (throw) "Throw rocks at the tree?"
        
        * (climb) "Climb the tree?"
        
    - &PORTRAIT_nick_nicknetural
    
    NICK: "..."
    
    &PORTRAIT_nour_nournervous
            
    NOUR: "Alright... Here goes."
    
    &FOLLOW_NOUR_NourFollowTreePos
    
    &PAUSE_0.5
    
    &FDEOUT
    
    "..."
    
    &SOUND_leavesrustling
    
    <i>Rustle rustle...</i> //ALSO SFX
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    {shake: 
    
        MS. SUWAN: "You shook it far too lightly."
        
    }
    
    {throw:
    
        MS. SUWAN: "You <i>barely</i> grazed the tree, Nour."
        
    }
    
    {climb:
    
        MS. SUWAN: "Nour, you <i>barely</i> went up the tree."
        
    }
    
    &PORTRAIT_nour_noursad
    
    NOUR: "I tried my best... but I couldn't do it."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "What now? I think I'm out of my element here."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "I..."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "I'm not sure either."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "If only we had someone who could do this REALLY well..."
    
    &PORTRAIT_nick_nickneutral

    NICK: "...I'll do it."
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "Really?"
    
    &PORTRAIT_nick_nickneutral
    
    NICK: "...Sure."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: <i>I got him speaking a little!</i>
    
    &PORTRAIT_nour_noursmile
    
    NOUR: <i>Hopefully this boosts his confidence...</i>
    
    ...
    
    //RUSTLING SFX
    
    &SOUND_leavesrustling
    
    <i>Rustle rustle...</i>
    
    &SHLORP_APPLE_GrandDuchessGranny_in
    
    &FDEIN
    
    NOUR: "Good job—"
    
    &PORTRAIT_ms. suwan_realization
    
    MS. SUWAN: "!"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "Woah! What the—"
    
    &PORTRAIT_???_grannysmile
    
    &SOUND_grandduchessgrannylaugh
    
    ???: "KEKEKE!"
    
    &PORTRAIT_ms. suwan_mssuwanrealization
    
    MS. SUWAN: "Is that one of those—" 
    
    &PAUSE_1.0

    &MUSIC_ApleQuestMusic
    
    &PORTRAIT_???_grannyneutral
    
    ???: "Thank the Mother Tree! Someone has awoken me from my slumber..."
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "...!"
    
    &PORTRAIT_???_grannysmile
    
    ???: "Ah, yes! What a handsome smile!"
    
    &PORTRAIT_grand duchess granny_grannyneutral
    
    GRAND DUCHESS GRANNY: "I, <i>GRAND DUCHESS GRANNY SMITH</i>, can tell with my old applady eyes..."
    
    &PORTRAIT_grand duchess granny_grannykiss
    
    GRAND DUCHESS GRANNY: "...That you are indeed a SWEETIEPIE!"
    
    &PORTRAIT_nick_nickangry
    
    NICK: "Um... what the heck?"
    
    &PORTRAIT_grand duchess granny_grannyangry
    
    GRAND DUCHESS GRANNY: "No frowning, young man! You young folks these days..."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "..."
    
    &PORTRAIT_grand duchess granny_grannyangry
    
    GRAND DUCHESS GRANNY: "Lacking CONFIDENCE? Lacking RESOLVE? Lacking SELF-ESTEEM?" 
    
    &PORTRAIT_grand duchess granny_grannyneutral
    
    GRAND DUCHESS GRANNY: "To that, I say..."
    
    &PORTRAIT_grand duchess granny_grannyangry
    
    GRAND DUCHESS GRANNY: "<i>PATOOEY!</i>"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "How... how'd you—"
    
    GRAND DUCHESS GRANNY: "I'm OLD, not BLIND for fruit's sake!"
    
    &PORTRAIT_grand duchess granny_grannyneutral
    
    GRAND DUCHESS GRANNY: "YOU were the one who got me out of this tree..."
    
    &PORTRAIT_grand duchess granny_grannyangry
    
    GRAND DUCHESS GRANNY: "Certainly not the other two USELESS bipeds with you!"
    
    &PORTRAIT_grand duchess granny_grannykiss
    
    GRAND DUCHESS GRANNY: "You succeeded <i>beautifully</i> where they FAILED."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Ouch... The truth hurts."
    
    &PORTRAIT_ms. suwan_mssuwanrealization
    
    MS. SUWAN: "... Indeed. She isn't wrong."
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "..."
    
    &PORTRAIT_grand duchess granny_grannyneutral
    
    GRAND DUCHESS GRANNY: "The Mother Tree knows and nurtures all."
    
    &PORTRAIT_grand duchess granny_grannysmile
    
    GRAND DUCHESS GRANNY: "A sweet seedling like you has the means to blossom..."
    
    &PORTRAIT_grand duchess granny_grannyangry
    
    GRAND DUCHESS GRANNY: "But only if <i>YOU</i> step into the sun!"
    
    &PORTRAIT_grand duchess granny_grannysmile
    
    GRAND DUCHESS GRANNY: "Plant your roots firmly, and reach for the sky!"
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "..."
    
    &PORTRAIT_grand duchess granny_grannyangry
    
    GRAND DUCHESS GRANNY: "<i>UNDERSTOOD, YOUNG HUMAN BOY?</i>"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "I-I, I mean, uh, YES?"
    
    &PORTRAIT_grand duchess granny_grannysmile
    
    &SOUND_grandduchessgrannylaugh
    
    GRAND DUCHESS GRANNY: "KEKEKE! <i>Very</i> good."
    
    &PORTRAIT_grand duchess granny_grannyneutral
    
    GRAND DUCHESS GRANNY: "And same goes for you two seedlings!"
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "Um, okay?!"
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "Seedling? I'm not nearly as young as these two."
    
    &PORTRAIT_grand duchess granny_grannyangry
    
    GRAND DUCHESS GRANNY: "AGE has nothing to do with the tangled roots in your hearts!"
    
    &PORTRAIT_grand duchess granny_grannyneutral
    
    GRAND DUCHESS GRANNY: "The Mother Tree connects us for a reason."
    
    GRAND DUCHESS GRANNY: "I pray that your time spent Between the Planes..."
    
    GRAND DUCHESS GRANNY: "... will unravel your haunted past."
    
    &PORTRAIT_grand duchess granny_grannysmile
    
    &SOUND_grandduchessgrannylaugh
    
    GRAND DUCHESS GRANNY: "KEKEKE! Now... what was I doing in here?"
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "I believe you were searching for the Prince?"
    
    &PORTRAIT_grand duchess granny_grannyneutral
    
    GRAND DUCHESS GRANNY: "Ah, the Prince..."
    
    &PORTRAIT_grand duchess granny_grannyangry
    
    GRAND DUCHESS GRANNY: "ANOTHER IDIOT BOY who doesn't KNOW HIS WORTH!"
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "... We've been sent by the Emperor to retrieve the Prince's search council."
    
    &PORTRAIT_grand duchess granny_grannyneutral
    
    GRAND DUCHESS GRANNY: "EVERCRISP! That mushy, sentimental fruit..."
    
    &PORTRAIT_grand duchess granny_grannysmile
    
    GRAND DUCHESS GRANNY: "Thank you for your hard work, seedlings."
    
    &PORTRAIT_nick_nickneutral
    
    NICK: "..."
    
    Nick nods his head.
    
    &PORTRAIT_grand duchess granny_grannyneutral
    
    GRAND DUCHESS GRANNY: "AH, yes! And one last thing..."
    
    &PORTRAIT_grand duchess granny_grannysmile
    
    GRAND DUCHESS GRANNY: "A parting gift! Some old wisdom from Granny..."
    
    &PORTRAIT_grand duchess granny_grannyneutral
    
    GRAND DUCHESS GRANNY: "<i>Give the ENEMY your greatest treasure...</i>"
    
    &PORTRAIT_grand duchess granny_grannysmile
    
    GRAND DUCHESS GRANNY: "<i>The SEED SEAL of the MOTHER TREE!</i>"
    
    &PORTRAIT_grand duchess granny_grannykiss
    
    GRAND DUCHESS GRANNY: "... And don't forget a kiss from Granny, too! KEKEKE!"
    
    &PORTRAIT_grand duchess granny_grannykiss
    
    //SFX GROSS WET KISSY
    
    &SOUND_wetkiss
    
    Grand Duchess Granny Smith blows Nick a fat kiss! 
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "Eek!"
    
    &PORTRAIT_grand duchess granny_grannysmile
    
    GRAND DUCHESS GRANNY: "KEKEKE!"
    
    GRAND DUCHESS GRANNY: "Send me away to the Emperor, dearies!"
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "Sure thing."
    
    &SOUND_grandduchessgrannylaugh
    
    GRAND DUCHESS GRANNY: "KEKEKE! GOODBYE, PRECIOUS ONES! <i>VISIT GRANNY SOMETIME, YES?</i>"
    
    &SHLORP_APPLE_GrandDuchessGranny_out
    
    &PAUSE_0.75
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "..."
    
    Nick touches his cheek. 
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "Good work getting her out of there!"
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "I would've never been able to do that without you."
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "..."
    
    Nick stays silent, but he nods his head again.
    
    &MUSIC_STOP
    
    NOUR: <i>He's responding more! We should continue the search...</i>
    
    &PAUSE_1.0
    
    &MUSIC_HallwayMusic
    
    &FOLLOW_NOUR_stop
    
    -> END
