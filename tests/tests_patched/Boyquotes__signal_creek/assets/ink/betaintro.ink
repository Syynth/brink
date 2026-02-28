=== betaintro_main1 ===
    //IN REALITY
    //location: elevator
    //NICK AND NOUR RUN INTO THE ELEVATOR
    
    &SPEED_NICK_120.0
    &SPEED_NOUR_100.0
    &FOLLOW_NICK_NickElevatorPos
    &FOLLOW_SUWAN_SuwanStartingPos
    &POS_SUWAN_160,248
    &FOLLOW_NOUR_NourStartingPos
    
    &PORTRAIT_nick_nickangry
    
    NICK: “HURRY UP, NOUR! There's no way she'll find us if we hide in here!"
    
    &PAUSE_0.5
    
    &EMOTE_NICK_DownIdle
    
    &FOLLOW_NOUR_NourElevatorPos
    &EMOTE_NICK_LeftIdle
    
    &PAUSE_0.5
    
    &EMOTE_NOUR_RightIdle
    
    &PORTRAIT_nour_nourangry
    
    NOUR: “In the BROKEN ELEVATOR?!”
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "Explain to me why this was a good idea."
    
    &SOUND_InquisitiveSound

    &SPEED_NICK_72.0
    &SPEED_NOUR_72.0
    
    &PORTRAIT_nour_nournervous
    
    // &PORTRAIT_nour_placeholder
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "Uh..."
    
    &PORTRAIT_nour_nourangry
    
        * "We have to get out of here!"
    
            NICK: "No way! Then, she's gonna—"
            
            &FOLLOW_SUWAN_NOUR
            
        * "This is what I get for listening to a stupid kid!"
        
            &PORTRAIT_nour_nourangry
            
            &PORTRAIT_nick_nickangry
    
            NICK: "Hey! You listening to me is technically <i>YOUR</i> fault, not mine."
            
            NICK: "I didn't <i>FORCE</i> you to follow me in here!"
    
            NICK: "And I'm NOT stupid—"
            
            
        * "Shhh! Shut up, Nick!"
    
            NICK: "NO, <i>YOU</i> SHUT UP!"
            
            &PORTRAIT_nour_nourangry
    
            NOUR: "If you keep making so much noise, she's going to <i>find us</i>, you idiot—"
            
            
    - MS. SUWAN: "Stop right there."
    
    &FOLLOW_SUWAN_SuwanElevatorPos
    
    &EMOTE_NOUR_DownIdle
    
    // &VOLUME_-80.0
    
    &PAUSE_1.0
    
    &EMOTE_NICK_LookAround
    
    &EMOTE_NICK_DownIdle
    
    &EMOTE_SUWAN_UpIdle
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "Aw, shoot! The scary old lady caught up!"
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "What did you just call me?"
    
    &EMOTE_NOUR_RightIdle
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "Nick, stop talking!"
    
    &EMOTE_NOUR_DownIdle
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "Both of you are trespassing. That could-"
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "... Wait."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: <i>"Nour?!"</i>
    
    &PORTRAIT_nour_nourshocked
    
    NOUR: "Uhh... Hi, Ms. Suwan."
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "How <i>dare</i> you disobey me?!"
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "I told you that you were <i>not</i> to step foot in this building!"
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "You know very well that it is unsafe."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "Not to mention, your trespassing could lead to <i>serious</i> trouble with the law."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: “Well, what are YOU doing here in that case?”
    
    &PORTRAIT_ms. suwan_mssuwanangry

    MS. SUWAN: "I AM the law."
    
    &EMOTE_NICK_LookAround
    &PORTRAIT_nick_nicktears
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "OH GOD— I'm so sorry! Please don't tell my Mom!"
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "I am leading the legal team responsible for inspecting this demolition site."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "As <i>Nour</i> knows very well."
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "Nour, how could you bring a <i>CHILD</i> here?!"
    
    &EMOTE_NICK_DownIdle
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "I-I, it— Look, it's none of your business, okay!"
    
    &PORTRAIT_nour_nournervous

    NOUR: "I <i>told</i> you— if I came, it would just be to take some photos for my project!"
    
    &PORTRAIT_nour_noursad
    
    NOUR: "I'll take him and leave the second I'm finished, you <i>know</i> I will, I-"
    
    &FOLLOW_NICK_NickButtonPressingPos
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "Save your excuses."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "Regardless of our professional relationship, Nour..."
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "... I will have to report <i>both</i> of you to law enforcement."
    
    &EMOTE_NICK_ButtonPress
    
    &EMOTE_SUWAN_DownIdle
    
    &SOUND_clack
    
    Nick begins frantically pressing elevator buttons.
    
    //&SOUND_ElevatorButtons
    
    &SOUND_clack
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "C'mon, c'mon, c'mon!"
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "You're an elevator! Get us anywhere BUT here!"
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "Seriously, Nick?! Pushing those isn't going to work!"
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "She's right. This elevator is broken. It's not going anywhere—"
    
    &SOUND_clickydoor
    
    &ELEVATOR_CLOSE
    
    //&SOUND_ElevatorDoorClose
    
    &PAUSE_0.3
    
    &SOUND_elevatorup
    
    &SHAKE
    
    &FOLLOW_SUWAN_SuwanBackup
    
    &PAUSE_1.0
    
    &EMOTE_SUWAN_DownIdle
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "What the—WHAT ARE YOU DOING?!"
    
    &PORTRAIT_nick_nickangry
    
    NICK: "SAVING OUR BUTTS!!!"
    
    &FOLLOW_NICK_NickRetreat
    
    &EMOTE_NOUR_LookAround
    
    &PAUSE_1.0
    
    &PORTRAIT_nour_nourangry
    
    NOUR: "WITH the person we're running from?!"
    
    &EMOTE_NICK_LookAround
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "Quiet down! This elevator shouldn't be working at all—"
    
    &EMOTE_SUWAN_DownIdle
    
    &PORTRAIT_nick_nicknervous
    
    &SOUND_shlorpbackwards
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "Why isn't it STOPPING? WHAT'S GOING ON—"
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "I don't know but... BRACE YOURSELVES!"
    
    &HIDEPANEL
    
    &BLACK
    
    &SOUND_elevatorcrash
    
    &PAUSE_10.0
    
    &FOLLOW_NOUR_stop
    
    &FOLLOW_NICK_NOUR
    
    &FOLLOW_SUWAN_NICK
    
    &FDEIN
    
    &EMOTE_NICK_DownIdle
    
    &EMOTE_NOUR_DownIdle
    
    &SOUND_clickydoor
    
    &ELEVATOR_OPEN
    
    -> END