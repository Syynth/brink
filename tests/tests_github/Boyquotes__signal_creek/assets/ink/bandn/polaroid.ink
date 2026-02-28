== polaroid ==

    A Polaroid One Step camera sits inside a glass case.
    
    { -!polaroid_first_visit:
        -> polaroid_first_visit
    }
    //convo after the first time PCs see Rina
    
    &FOLLOW_NOUR_NourMoveAway
    &FOLLOW_SUWAN_SuwanPolaroid
    &PAUSE_1.0
    &EMOTE_NOUR_RightIdle
    &EMOTE_SUWAN_UpIdle
    
    
    NOUR: "Look, I said I don't want to look at this right now." // (ANGRY)
    
    MS. SUWAN: "Fine, we'll come back later then."
    
    -> END
    
== polaroid_first_visit ==
    The camera is surrounded by a bunch of film photographs of two girls. 
    
    One of them sits on the floor outside of the case. 
    
    &FOLLOW_NOUR_NourPolaroid
    
    &FOLLOW_NICK_NickPolaroid
    
    &FOLLOW_SUWAN_SuwanPolaroid
    
    &PAUSE_1.0
    
    &EMOTE_NOUR_UpIdle
    
    &EMOTE_NICK_LeftIdle
    
    &EMOTE_SUWAN_RightIdle
    
    &PORTRAIT_nour_nourshocked
    
    NOUR: "...No way." // (SHOCKED)
    
    NICK: "Hm?"
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "I think that's my dad's old polaroid. Well, <i>my</i> polaroid, technically."
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "Like... <i>your</i> camera your camera or the same <i>kind</i> of camera, your camera?"
    
    NOUR: "I'm not sure, but probably not <i>my</i> camera."
    
    NOUR: "That would be impossible... Right?"
    
    &PORTRAIT_nick_nickneutral
    
    NICK: "Right. Completely impossible."
    
    NICK: "Just like us being trapped here right now is impossible."
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "..." // (ANGRY)
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "Aaaanyways..." 
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "Retro stuff is super trendy now, y'know. Did you use it a lot?"
    
    &FOLLOW_NOUR_NourMoveAway
    
    &PAUSE_0.1
    
    &EMOTE_NOUR_UpIdle
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "Yeah, kind of. It's... it's whatever." // (NERVOUS)
    
    Nour tries to put the polaroid back on the table, but Ms. Suwan grabs it before it reaches the table.
    
    &FOLLOW_SUWAN_NourPolaroid
    
    &PAUSE_0.1
    
    &EMOTE_SUWAN_UpIdle
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "Wait Nour, there are pictures of you in this polaroid."
    
    NOUR: "What are you talking about?"
    
    MS. SUWAN: "Take a look for yourself."
    
    &FOLLOW_SUWAN_SuwanPolaroid
    
    &FOLLOW_NOUR_NourPolaroid
    
    &PAUSE_0.1
    
    &EMOTE_NOUR_UpIdle
    
    &PAUSE_0.175
    
    &EMOTE_NOUR_Jump
    
    &PAUSE_2.0
    
    &EMOTE_NOUR_LeftIdle
    
    &EMOTE_SUWAN_RightIdle
    
        * "I don't know..." // (NERVOUS)
            MS. SUWAN: "Are you sure? Did you put them here?"
            
            &PORTRAIT_ms. suwan_mssuwanannoyed
            
            MS. SUWAN: "Are you trying to prank us, young lady?" // (ANGRY)
            
            &PORTRAIT_nour_nournervous
        
            NOUR: "No! I really don't know, ma'am!"
            
            NOUR: "I haven't been here before this is the first time, I swear!" // (SHOCKED)
            
            &PORTRAIT_ms. suwan_mssuwanannoyed
        
            MS. SUWAN: "I thought I could believe you before, but..." // (SQUINTING)
    
        * "Maybe... it has to do with... the girl at the elevator..." // (NERVOUS)
    
            MS. SUWAN: "Do you know her? Did you come here with her?"
            
            &PORTRAIT_ms. suwan_mssuwangry
            
            MS. SUWAN: "Are you setting something up, young lady?" // (ANGRY)
            
            &PORTRAIT_nour_nournervous
        
            NOUR: "What? No! I'm not setting anything up." 
            
            NOUR: "This is the first time I've seen this, I swear!" // (SHOCKED)
            
            &PORTRAIT_ms. suwan_mssuwanrealization
        
            MS. SUWAN: "But you know that girl, right?"
            
            &PORTRAIT_nour_nourshadow
        
            NOUR: "..."
        
            NOUR: "I didn't come here with her, I promise." 
            
            &PORTRAIT_ms. suwan_mssuwanangry
        
            MS. SUWAN: "I want to believe you, but it is getting rather more difficult to..." // (SQUINTING)
    
        * "Someone has to be messing with us." 
    
            MS. SUWAN: "Perhaps it could be the girl we saw earler?"
            
            MS. SUWAN: "She seemed to have some strange knowledge of the goings-on here."
        
            NOUR: "...Maybe."
    
    - MS. SUWAN: "Hm... Well, regardless, I <i>will</i> have to report this to my higher ups."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "Maybe all of this is someone's secret hide-out." // (SQUINTING)
    
    &FOLLOW_NOUR_NourMoveAway
    
    &PAUSE_1.0
    
    &EMOTE_NOUR_DownIdle
    
    NOUR: "Yeah, sure. Look, let's just move on, okay?" // (NERVOUS)
    
    MS. SUWAN: "..."
    
    &PORTRAIT_ms. suwan_mssuwanrealization
    
    &FOLLOW_SUWAN_NourPolaroid
    
    &PAUSE_0.1
    
    &EMOTE_SUWAN_UpIdle
    
    MS. SUWAN: "...But it <i>is</i> strange to find photographs of yourself in a place like this regardless."
    
    MS. SUWAN: "Who is it that you're with? In the polaroids?"
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "I can't quite make out their face."
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "Or it's magic hiding them! Can I see?" // (HAPPY)
    
    MS. SUWAN: "No. Nour?" 
    
        * "I said we need to move on."
        
            &PORTRAIT_ms. suwan_suwanannoyed
        
            MS. SUWAN: "You're not giving me any reason to believe you aren't hiding something."
            
            &PORTRAIT_nour_nourshadow
        
            NOUR: "W-well then, think whatever you want! I don't want to talk about it." // (ANGRY)
            
            &PORTRAIT_ms. suwan_mssuwanangry
    
            MS. SUWAN: "...Hmph. Fine. We'll move on because we must, <i>for now</i>." 
            
            MS. SUWAN: "We'll come back to this later, though, don't think I'll forget about it."
        
            Ms. Suwan places the photograph on top of the glass case.
        
            MS. SUWAN: "You <i>will</i> have to face up to whatever happened sooner or later."
            
            &PORTRAIT_nour_nourshadow
        
            NOUR: "Hmph. I don't see <i>why</i> when I didn't do anything wrong..."
            
            &PORTRAIT_nour_nourneutral
            
            NOUR: "Whatever. Let's go." // (ANGRY)
        
            &EMOTE_NICK_LeftIdle
            
            Nick leans in and whispers to Ms. Suwan. 
            
            &PORTRAIT_nick_nickworried
        
            NICK: "I think she did something she's not happy about." 
            
            &EMOTE_SUWAN_RightIdle
        
            MS. SUWAN: "We've all done things that might have been in poor judgement."
            
            MS. SUWAN: "That does not mean we should hide from our choices."
            
            MS. SUWAN: "But let's keep looking around for now."
            
            &PORTRAIT_nour_nourannoyed
            
            &EMOTE_NOUR_UpIdle
        
            NOUR: "I can <i>hear</i> you, y'know." // (ANGRY)
            
            &EMOTE_NICK_DownIdle
            
            &EMOTE_SUWAN_DownIdle
            
            &PORTRAIT_nick_nicknervous
        
            NICK: "No you can't! I'm whispering!" 
            
            NICK: "It's rude to listen on to someone whispering!" // (ANGRY)
            
            &EMOTE_NICK_UpIdle
            
            &EMOTE_SUWAN_UpIdle
            
        * "... She's an old friend..."
        
            &PORTRAIT_nour_nourshadow
        
            NOUR: "I... cared about her, a lot." 
            
            NOUR: "But we don't talk anymore. It doesn't matter." // (SAD)
            
            &PORTRAIT_nick_nicksmallsmile
        
            NICK: "It does though! How did you meet her?"
            
            NICK: "Did you know her for a long time? What's her name?"
            
            NICK: "Why don't you talk anymore?" 
            
            &PORTRAIT_nour_nourannoyed
        
            NOUR: "I'm answering none of those, and you're going to stop asking questions." // (ANGRY)
            
            &PORTRAIT_nick_nicksad
        
            NICK: "Aww, but I wanna know! Why don't you wanna talk about her?"
            
            NICK: "What if <i>I</i> want to be her friend?"
            
            &PORTRAIT_nour_nourangry
        
            NOUR: "What did I <i>just</i> say?" // (ANGRY)
            
            &PORTRAIT_ms. suwan_mssuwanannoyed
        
            MS. SUWAN: "Children, enough."
            
            &PORTRAIT_nour_nourannoyed
            
            NOUR: "I'm not a <i>child</i>." // (ANGRY)
            
            &PORTRAIT_nick_nickannoyed
        
            NICK: "Neither am I!" // (ANGRY)
            
            &PORTRAIT_nour_nourangry
        
            NOUR: "Yes you <i>are</i>. You're not even out of highschool." // (ANGRY)
            
            &PORTRAIT_ms. suwan_mssuwanfurious
        
            MS. SUWAN: "<i>Children.</i>" 
            
            MS. SUWAN: "<i>Quiet</i>."
            
            &EMOTE_SUWAN_RightIdle
            
            &PORTRAIT_mssuwan_mssuwanangry
            
            MS. SUWAN: "Nick, stop asking Nour questions about something she clearly does not want to talk about it."
            
            &PORTRAIT_nick_nicksad
        
            NICK: "Come <i>onnnn</i>." // (POUTING) 
        
            NICK: "Fine. <i>For now</i>."
            
            &PORTRAIT_nour_nourshadow
        
            NOUR: "..." // (ANGRY)
            
            &EMOTE_SUWAN_DownIdle
        
            MS. SUWAN: "And Nour? We'll come back to this later."
            
            MS. SUWAN: "Whatever happened, you're going to have to face up to it at some point."
            
            &PORTRAIT_nour_nourangry
        
            NOUR: "I don't see why I should. It was long ago." 
            
            &PORTRAIT_nour_nourannoyed
            
            &EMOTE_NOUR_DownIdle
            
            NOUR: "Whatever. Let's just go." // (ANGRY)
            
            &EMOTE_NICK_UpIdle
            
            &EMOTE_SUWAN_UpIdle
            
        - &MOV_RINA_BANDN
            
        &PAUSE_0.5
        
        &PORTRAIT_nour_nourshocked
        
        NOUR: <i>She's... here? Again?</i> // (SHOCKED)
        
        &PORTRAIT_nour_nourannoyed
        
        NOUR: <i>I better tell her to get out of our way.</i> // (ANGRY)
        
         &FOLLOW_NOUR_stop
        
        -> rina_bandn
