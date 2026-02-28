== fountain ==

    It's a fountain with some pennies and other stuff inside. 
    
    &FOLLOW_NICK_NickFountainPos
    &FOLLOW_SUWAN_SuwanFountainPos
    &FOLLOW_NOUR_NourFountainPos
    
    &PAUSE_1.0
    
    &EMOTE_NICK_DownIdle
    &EMOTE_SUWAN_DownIdle
    &EMOTE_NOUR_DownIdle
    
    &FOLLOW_NOUR_stop
    
    {
        - fountain_findfather:
            -> explored_everything
            
        - consoles_start_aplequest: 
            -> fountain_findfather
            
        - fountain_before_aplequest:
            -> fountain_before_aplequest_repeat
            
        - else:
            -> fountain_before_aplequest
    }

== fountain_before_aplequest ==

    &PORTRAIT_nour_nourpondering

    NOUR: "Got a penny to make a wish?"
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "Nope. Who even carries around pennies anymore?"
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "Good point."
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "Putting your faith in silly things like wishes is a waste of time."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "You need to do good work and know the right people."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "So, like, everything is impossible? God..."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "That's a little depressing... But also true, to some extent."
    
    -> END

== fountain_before_aplequest_repeat ==

    &PORTRAIT_nour_nourneutral

    NOUR: <i>I've checked this already.</i> 
    
    &PORTRAIT_nour_nourpondering

    NOUR: <i>There's something curious in the water...</i>
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>But I should stay focused on finding a way out.</i>
    
    -> END

== fountain_findfather ==

    &PAUSE_1.0
    
    &AUDIO_FADEOUT

    &PORTRAIT_nour_nourpondering

    NOUR: "Hey... Is there something bobbing up and down in the water?"
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "Just trash and debris."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "No, there's something red inside."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "Do you think it could be an..."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "There's only one way to find out."
        
        * ["Let's fish it out of the water."]

    - &PORTRAIT_nour_nourpondering
    
    NOUR: "..."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "...Hey!"
    
    // &SHLORP_POOP_FatherFuji_in
    
    &EMOTE_NOUR_Jump
    &EMOTE_NICK_Jump
    &EMOTE_SUWAN_Jump
    
    &PAUSE_0.5
    
    &EMOTE_NICK_DownIdle
    &EMOTE_SUWAN_DownIdle
    &EMOTE_NOUR_DownIdle
    
    &PORTRAIT_???_fatherfujineutral
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "...!"
    
    &SOUND_fatherfujilaugh
    
    ???: "Greetings."
    
    ???: "I see the ones that have rescued me hail from the Mother's Garden."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "The ones from the where now?"
    
    &PAUSE_1.0

    &MUSIC_ApleQuestWaterMusic
    
    &PORTRAIT_???_fatherfujipreach
    
    ???: "Be not confused, gentle human. The Mother Tree forgives everyone."
    
    &PORTRAIT_???_fatherfujismile
    
    ???: "For we are all Children of the Fruit."
    
    &PORTRAIT_father fuji_fatherfujipreach
    
    FATHER FUJI: "I am the insignificant <i>FATHER FUJI</i>..."
    
    &PORTRAIT_father fuji_fatherfujineutral
    
    FATHER FUJI: "...Head priest of the Aple Council."
    
    &PORTRAIT_father fuji_fatherfujipreach
    
    FATHER FUJI: "I take it that the earthly Emperor Evercrisp tasked you with finding me?"
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "Yes. We're looking for all the missing members— including your Prince."
    
    &PORTRAIT_father fuji_fatherfujineutral
    
    FATHER FUJI: "Ah... Yes. The Prince. How noble his core is, from stem to seed."
    
    &PORTRAIT_father fuji_fatherfujisad
    
    FATHER FUJI: "But unfortunately, he has strayed and fallen far from the tree." 
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "Care to explain more about this Prince of yours?"
    
    &PORTRAIT_father fuji_fatherfujipreach
    
    FATHER FUJI: "Since the invasion, our Prince has lost much self-esteem."
    
    &PORTRAIT_father fuji_fatherfujineutral
    
    FATHER FUJI: "Sent by his father, the Emperor, to Earth was he."
    
    &PORTRAIT_father fuji_fatherfujineutral
    
    FATHER FUJI: "No fault by him or his fights, but for his safety."
    
    &PORTRAIT_father fuji_fatherfujisad
    
    FATHER FUJI: "Yet the young Prince saw it as failing his Daddy."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "...Daddy?"
    
    &PORTRAIT_father fuji_fatherfujipreach
    
    FATHER FUJI: "Rhyming on command is harder than it looks."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "Of course. Continue, please."
    
    &PORTRAIT_father fuji_fatherfujineutral
    
    FATHER FUJI: "Forced from space to Earth on a hyper-speed ride,"
    
    &PORTRAIT_father fuji_fatherfujipreach
    
    FATHER FUJI: "The prince hid in shame, with wounded pride."
    
    &PORTRAIT_father fuji_fatherfujineutral
    
    FATHER FUJI: "Called by his people, for help, they cried,"
    
    &PORTRAIT_father fuji_fatherfujisad
    
    FATHER FUJI: "But the Prince fell silent as his confidence died."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "I see. That's a pretty sad story..."
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "What do you think, Nick?"
    
    &PORTRAIT_nick_nickshadow
    
    NICK: "...Whatever."
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>How do I get him to care?...</i>
    
    &PORTRAIT_father fuji_fatherfujineutral
    
    FATHER FUJI: "Now that you've heard this tale of woe..."
    
    &PORTRAIT_father fuji_fatherfujipreach
    
    FATHER FUJI: "I will impart aple wisdom before you go."
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "What's that?"
    
    &PORTRAIT_father fuji_fatherfujineutral
    
    FATHER FUJI: "A piece of information a Plane Walker ought to know."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "...Information? 
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "..."
    
    Nick shuts his mouth.
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: <i>He responded to Father Fuji!</i>
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>I should encourage him to talk more... but how?</i>
    
    &PORTRAIT_nour_nourneutral
    
        * "Sounds pretty important."
        
            &PORTRAIT_nour_nourrealization
        
            NOUR: "Too bad I don't know much about this game for it to make sense!"
            
            &PORTRAIT_nick_nicknervous
            
            NICK: "...!"
            
            &PORTRAIT_nour_nourpondering
            
            NOUR: "But maybe ONE of us will understand it..."
            
            &PORTRAIT_nour_noursmile
            
            NOUR: "So I guess we'll hear you out."
            
        * "Actually, we're pretty busy."
        
            &PORTRAIT_nour_noursad
            
            NOUR: "I think we'll pass on this very important piece of information."
            
            &PORTRAIT_nick_nickshocked
            
            NICK: "...!"
            
            &PORTRAIT_nick_nicknervous
            
            NICK: "Bu-!" 
            
            &PORTRAIT_nour_nourpondering
            
            NOUR: "I thought you didn't care to play this game with us, Nick."
            
            &PORTRAIT_nick_nicksad
            
            NICK: "...I <i>don't</i> care."
            
            &PORTRAIT_ms. suwan_mssuwanrealization
            
            MS. SUWAN: "..."
            
            &PORTRAIT_ms. suwan_mssuwansad
            
            MS. SUWAN: "You don't care... Hmm."
            
            &PORTRAIT_ms. suwan_mssuwanpondering
            
            MS. SUWAN: "Let's just hear this out to the end."
            
            &PORTRAIT_nick_nickshocked
            
            NICK: "...!"
    
    - Nick lets out a faint sigh of relief.
    
    &PORTRAIT_nour_noursmile
    
    NOUR: <i>Looks like he's letting down his guard a little more.</i>
    
    &PORTRAIT_father fuji_fatherfujineutral
    
    FATHER FUJI: "While thy prince was both kept and fair,"
    
    &PORTRAIT_father fuji_fatherfujisad
    
    FATHER FUJI: "His companions oft did mock and stare."
    
    &PORTRAIT_father fuji_fatherfujipreach
    
    FATHER FUJI: "Forsooth they jested at his unwist gait,"
    
    &PORTRAIT_father fuji_fatherfujineutral

    FATHER FUJI: "Hoping this majesty's unripnes would abate."
    
    &PORTRAIT_father fuji_fatherfujipreach
    
    FATHER FUJI: "Though prince did strive to laugh it off,"
    
    &PORTRAIT_father fuji_fatherfujineutral
    
    FATHER FUJI: "And pretend that he was just as tough,"

    &PORTRAIT_father fuji_fatherfujisad

    FATHER FUJI: "Deep inside he felt so small and weak,"
    
    &PORTRAIT_father fuji_fatherfujipreach

    FATHER FUJI: "Longing for a friend who wouldn't critique."
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "..."
    
    &PORTRAIT_father fuji_fatherfujismile
    
    FATHER FUJI: "But found he, a true friend, and thus had sworn,"
    
    &PORTRAIT_father fuji_fatherfujipreach
    
    FATHER FUJI: "<i>To never Y-ATTACK the Prince of Corn!</i>"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "...!"
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>I don't get how that's important...</i>
    
    &PORTRAIT_nour_noursmile
    
    NOUR: <i>But I think Nick did!</i>
    
    &PORTRAIT_father fuji_fatherfujineutral
    
    FATHER FUJI: "Now my children..."
    
    &PORTRAIT_father fuji_fatherfujipreach
    
    FATHER FUJI: "May the Mother Tree guide thee..."
    
    &PAUSE_0.5
    
    &SHLORP_APPLE_FatherFuji_out
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Well, that made zero sense to me."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "Agreed. Perhaps it was more apple nonsense."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "I guess! Unless <i>someone</i> could explain it to us..."
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "..." 
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "Until then... let's move on."
    
    &MUSIC_STOP
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>Nick seems to be more invested. This is good.</i>
    
    &PORTRAIT_nour_noursmile
    
    NOUR: <i>I should find more apples to make him open up even more!</i>
    
    &PAUSE_1.0

    &MUSIC_HallwayMusic
    
    &FOLLOW_NOUR_stop
    
    -> END
