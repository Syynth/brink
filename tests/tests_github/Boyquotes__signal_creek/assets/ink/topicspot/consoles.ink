== consoles ==

{
    - consoles_afternickroute:
        -> explored_everything
        
    - nourplaysaplequest:
        -> consoles_afternickroute
        
    - wertoys_find_prince:
        -> nourplaysaplequest
        
    - consoles_start_aplequest:
        -> consoles_start_aplequest_repeat
        
    - aplequestguide_after_bookshelf_fightstarts:
        -> consoles_start_aplequest
        
    - consoles_before_start && consoles_before_aplequest:
        -> consoles_visited
        
    - consoles_before_start:
        -> consoles_before_aplequest
        
    - else: 
        -> consoles_before_start
}

=== consoles_before_start ===
    A start screen for a retro game called Aple Quest plays.
    
    &FOLLOW_NICK_NickConsole
    
    &FOLLOW_NOUR_NourLookAtNick
    
    &FOLLOW_SUWAN_SuwanLookAtNick
    
    &PAUSE_1.0
    
    &EMOTE_NICK_UpIdle
    
    &EMOTE_NICK_Jump
    
    &EMOTE_NOUR_UpIdle
    
    &EMOTE_SUWAN_UpIdle
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "<i>Aple Quest</i>? Here? In brand-new condition?"
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "Looks like it still runs just fine."
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "Man, I can't believe it's here in its original retro console!"
    
        * "You wanna give it a go?"
            
            &PORTRAIT_nick_nicksmallsmile
            
            NICK: "I—!"
            
            &PORTRAIT_nick_nicknervous
            
            &EMOTE_NICK_DownIdle
            
            &FOLLOW_NICK_NickStepAway
            
            &PAUSE_1.0
            
            NICK: "Um... no. This stuff's for little kids, so..."
            
            &PORTRAIT_nick_nicksad
            
            NICK: "I probably shouldn't."
            
        * "We shouldn't waste time..."
            
            &PORTRAIT_nour_nourneutral
        
            NOUR: "We should look for something that could help us."
            
            &PORTRAIT_nick_nicknervous
            
            &EMOTE_NICK_DownIdle
            
            &FOLLOW_NICK_NickStepAway
            
            &PAUSE_1.0
            
            NICK: "Um... yeah. This stuff's for little kids, so..."
            
            &PORTRAIT_nick_nicksad
            
            NICK: "We should just move on."
            
    &EMOTE_NOUR_DownIdle
    
    &EMOTE_SUWAN_DownIdle
    
    - &PORTRAIT_nour_nournervous
    
    NOUR: "... Hey, are you alright?"
    
    &PORTRAIT_nick_nickshadow
    
    &FOLLOW_NICK_NickGoesAway
    
    &PAUSE_1.0
    
    &EMOTE_NICK_UpIdle
    
    NICK: "I'm fine. Let's just go already."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "Whatever you say..."
    
    NOUR: <i>Weird. He seemed like he wanted to talk about this game.</i>
    
    NOUR: <i>I guess something about it is bothering him.</i>
    
    NOUR: <i>Maybe he'll want to talk about it later...</i>
    
    &FOLLOW_NOUR_stop
    
    &FOLLOW_NICK_NOUR
    
    &FOLLOW_SUWAN_NICK

    -> END

== consoles_before_aplequest ==

    The soft 8-bit boops of <i>Aple Quest</i> play, but Nick only stares at the screen.
    
    &FOLLOW_NICK_NickGoesAway
    
    &FOLLOW_NOUR_NourConsole
    
    &FOLLOW_SUWAN_SuwanConsole
    
    &PAUSE_1.0
    
    &EMOTE_SUWAN_UpIdle
    
    &EMOTE_NOUR_UpIdle
    
    &EMOTE_NICK_UpIdle
    
        &PORTRAIT_nour_nournervous
    
    NOUR: <i>Why isn't he saying anything? Maybe I have to start first...</i>
    
        &PORTRAIT_nour_noursmile
        
    &EMOTE_NOUR_DownIdle
    
    NOUR: "You know a lot about this game, right Nick?"
    
        &PORTRAIT_nick_nicknervous
    
    NICK: "Yeah. I don't know. Not really."
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    &EMOTE_SUWAN_DownIdle
    
    MS. SUWAN: "You said many words with little meaning."
    
    &PORTRAIT_nick_nickangry
    
    NICK: "I meant to say <i>no</i>! Just get off my case already, okay?"
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "No need to be defensive. It was not a personal attack."
    
        &PORTRAIT_nick_nicksad
        
    &EMOTE_NICK_DownIdle
    
    NICK: "Whatever..."
    
    Nick huffs and turns away from the game.
    
    &FOLLOW_NOUR_stop
    
    &FOLLOW_NICK_NOUR
    
    &FOLLOW_SUWAN_NICK
    
    -> consoles_visited

== consoles_visited ==

    &FOLLOW_NOUR_NourConsole
    
    &FOLLOW_SUWAN_SuwanConsole
    
    &FOLLOW_NICK_NickGoesAway
    
    &PAUSE_1.0
    
    &EMOTE_SUWAN_UpIdle
    
    &EMOTE_NOUR_UpIdle
    
    &EMOTE_NICK_UpIdle
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>Nick doesn't seem to want to talk about this yet.</i>
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>We'll come back when he's ready.</i>
    
    &EMOTE_NICK_DownIdle
    
    &PORTRAIT_nick_nickneutral
    
    NICK: "Can we look at the spray paint or something?"
    
    &FOLLOW_NOUR_stop
    
    &FOLLOW_NICK_NOUR
    
    &FOLLOW_SUWAN_NICK

    -> END

== consoles_start_aplequest_repeat ==

    &FOLLOW_NOUR_NourConsole
    
    &FOLLOW_SUWAN_SuwanConsole
    
    &FOLLOW_NICK_NickGoesAway
    
    &PAUSE_1.0
    
    &EMOTE_SUWAN_UpIdle
    
    &EMOTE_NOUR_UpIdle
    
    &EMOTE_NICK_UpIdle

    &PORTRAIT_nour_nourneutral

    NOUR: <i>Emperor Evercrisp said we've gotta find the four Aple Council members.</i>
    
    &PORTRAIT_nour_noursmile
    
    NOUR: <i>Once we find them, then we can find the Prince.</i>
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>They should be hidden somewhere in the hallway.</i>
    
    &FOLLOW_NOUR_stop
    
    &FOLLOW_NICK_NOUR
    
    &FOLLOW_SUWAN_NICK
    
    -> END

== consoles_start_aplequest ==

    &FOLLOW_NOUR_NourConsole
    
    &FOLLOW_SUWAN_SuwanConsole
    
    &FOLLOW_NICK_NickGoesAway
    
    &PAUSE_1.0
    
    &EMOTE_SUWAN_UpIdle
    
    &EMOTE_NOUR_UpIdle
    
    &EMOTE_NICK_UpIdle

        &PORTRAIT_nour_noursmile
        
    NOUR: "Look, Nick. It's the game we were reading about before."
    
        &PORTRAIT_nick_nicknervous
    
    NICK: "..."
    
        &PORTRAIT_nour_nournervous
        
    &EMOTE_NICK_DownIdle
    
    NOUR: "Do you want to try playing it?"
    
    Nick turns his head away, silent.
    
    &EMOTE_SUWAN_RightIdle
    
    MS. SUWAN: "Give it up, Nour. He's throwing a tantrum—"
    
    &SHLORP_poop_EmperorEvercrisp_in
    
        &PORTRAIT_???_emperorevercrispsmile
    
    ???: "NEVER GIVE UP!!!"
    
        &PORTRAIT_nour_nournervous
        
    &EMOTE_SUWAN_UpIdle
    
    &EMOTE_NOUR_UpIdle
    
    &EMOTE_NICK_UpIdle
    
    &PAUSE_0.1
    
    &EMOTE_SUWAN_Jump
    
    &EMOTE_NOUR_Jump
    
    &EMOTE_NICK_Jump
    
    NOUR: "What the—!"
    
    &PAUSE_1.0
    
    &AUDIO_FADEOUT
    
    The console screen glitches, and the game starts by itself...
    
    &PAUSE_1.0

    &MUSIC_ApleQuestMusic
    
    &PORTRAIT_???_emperorevercrisppreach

    ???: "The Council of Malus Domestica has fallen, and you are our only hope!"
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    &EMOTE_SUWAN_RightIdle
    
    MS. SUWAN: "Nour, what did you press?"
    
        &PORTRAIT_nour_nourannoyed
        
    &EMOTE_NOUR_LeftIdle
    
    NOUR: "I didn't press anything!"
    
    &PORTRAIT_???_emperorevercrispsmile
    
    ???: "Do not panic, for I am a friend—"
    
    &EMOTE_NICK_UpIdle
    
    &PORTRAIT_???_emperorevercrisppreach
    
    EMPEROR EVERCRISP: "I am <b>EMINENT EMPEROR EVERCRISP APLE</b>, leader of the Aples!"
    
    &PORTRAIT_nour_nournervous
    
    &EMOTE_NOUR_LeftIdle
    
    NOUR: "The game character is... speaking... to us?"
    
    &EMOTE_SUWAN_RightIdle
    
    &PORTRAIT_ms. suwan_mssuwanrealization
    
    MS. SUWAN: "This can't be real. This has to be some trick or prank."
    
    &PORTRAIT_emperor evercrisp_emperorevercrispsmile
    
    EMPEROR EVERCRISP: "Ho ho! Who is to say what is real and what is not?"
    
    &EMOTE_NOUR_UpIdle
    
    &EMOTE_SUWAN_UpIdle
    
    &PORTRAIT_emperor evercrisp_emperorevercrispneutral
    
    EMPEROR EVERCRISP: "For you three... Those Who Walk Between Planes..." 
    
    &PORTRAIT_emperor evercrisp_emperorevercrisppreach
    
    EMPEROR EVERCRISP: "The past, present, and future exist all at once!"
    
        &PORTRAIT_nour_nourpondering
    
    NOUR: "Those who walk between... what?"
    
    &PORTRAIT_emperor evercrisp_emperorevercrispneutral
    
    EMPEROR EVERCRISP: "Our <i>Prince</i> went to Earth in search of the <b>four</b> missing members to restore order..."
    
    &PORTRAIT_emperor evercrisp_emperorevercrispsad
    
    EMPEROR EVERCRISP: "... But we've lost all communication with him."
    
    &PORTRAIT_emperor evercrisp_emperorevercrisppreach
    
    EMPEROR EVERCRISP: "Please, Dream Walkers! Bring me the Prince and all the Apple Council members!"
    
    &PORTRAIT_emperor evercrisp_emperorevercrispsmile
    
    EMPEROR EVERCRISP: "I shall reward you handsomely!"
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "Dream Walkers?"
    
        &PORTRAIT_nour_nourpondering
    
    NOUR: "If we return with everyone, you'll reward us with what, exactly?"
    
    &PORTRAIT_emperor evercrisp_emperorevercrisppreach
    
    EMPEROR EVERCRISP: "Ho ho... How about access to our SECRET ENDING...?"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "... !"
    
    &PORTRAIT_emperor evercrisp_emperorevercrispsmile
    
    EMPEROR EVERCRISP: "Does this not entice you? HMM?"
    
    &PORTRAIT_nour_nourneutral
    
        * "Yeah."
        
                &PORTRAIT_nour_noursmile
                
            NOUR: "We have a big fan of yours over here, actually."
            
            &EMOTE_NOUR_DownIdle
            
            NOUR: "I think he'd love to get a Secret Ending, right?"
            
                &PORTRAIT_nick_nicknervous
            
            NICK: "..."
            
                &PORTRAIT_nour_noursad
            
            NOUR: "Um, that was a silent agreement."
            
            &PORTRAIT_emperor evercrisp_emperorevercrispsmile
            
            EMPEROR EVERCRISP: "HO HO! EXCELLENT, MARVELOUS, SPECTACULAR!!!"
            
            &EMOTE_NOUR_UpIdle
        
        * "Not really..."
        
            &PORTRAIT_emperor evercrisp_emperorevercrispsmile
            
            EMPEROR EVERCRISP: "HO HO! EXCELLENT—"
            
            &MUSIC_STOP
            
            &PORTRAIT_emperor evercrisp_emperorevercrispneutral
            
            EMPEROR EVERCRISP: "... Oh."
            
            &PORTRAIT_emperor evercrisp_emperorevercrispsad
            
            EMPEROR EVERCRISP: "Does a Secret Ending not motivate you enough?"
            
                &PORTRAIT_nour_nourpondering
                
            &EMOTE_NOUR_UpIdle
            
            NOUR: "Mmm... nope. I don't really know anything about <i>Aple Quest</i>."
            
            &PORTRAIT_nick_nickshocked
            
            NICK: "..."
            
                &PORTRAIT_nour_nourneutral
            
            NOUR: "I also don't know you. You could be a dangerous, uh, apple for all we know." 
            
            &PORTRAIT_emperor evercrisp_emperorevercrispneutral
            
            EMPEROR EVERCRISP: "OH... um..."
            
            &PORTRAIT_emperor evercrisp_emperorevercrisppreach
            
            EMPEROR EVERCRISP: "H-How would you like, say, a FREE BOX OF APLE QUEST SPACE-O's?"
            
            &PORTRAIT_nour_nournervous
            
            NOUR: "... Um. I don't know."
            
            &PORTRAIT_emperor evercrisp_emperorevercrispsad
            
            EMPEROR EVERCRISP: "Don't you want a delicious, nutritious breakfast cereal?"
            
            &PORTRAIT_emperor evercrisp_emperorevercrispsmile
            
            EMPEROR EVERCRISP: "IT'S SPACE <i>AND</i> APPLE THEMED!"
            
                &PORTRAIT_nour_nourpondering
            
            NOUR: "Eh..."
            
            &PORTRAIT_emperor evercrisp_emperorevercrispsad
            
            EMPEROR EVERCRISP: "P-Please?"
            
            &PORTRAIT_nick_nicknervous
            
            NICK: "..."
            
                &PORTRAIT_nour_nourrealization
            
            NOUR: "... You know what? Why not."
            
            &PORTRAIT_nour_noursmile
            
            NOUR: "We have an expert on our side anyway."
            
            &PORTRAIT_emperor evercrisp_emperorevercrispsmile
            
            EMPEROR EVERCRISP: "PHEW! You humans drive a hard bargain, ho ho!"
            
                &PORTRAIT_nour_nourneutral
            
            NOUR: "Oh, and I think we'll pass on the cereal. The ending's enough."
            
            &PORTRAIT_emperor evercrisp_emperorevercrisppreach
            
            EMPEROR EVERCRISP: "VERY COOL!"
            
            &PORTRAIT_emperor evercrisp_emperorevercrispsmile
            
            EMPEROR EVERCRISP: "Because I don't know where I put that stuff, ho ho!"
            
            &MUSIC_ApleQuestMusic
            
    - &PORTRAIT_nour_nourpondering
    
    NOUR: "So all we have to do is find four apple... People... And then the Prince?"
    
    &PORTRAIT_emperor evercrisp_emperorevercrisppreach
    
    EMPEROR EVERCRISP: "If you want that sweet, sweet secret ending!"
    
    &PORTRAIT_emperor evercrisp_emperorevercrispsad
    
    EMPEROR EVERCRISP: "But yes, please. I miss my son very much..."
    
        &PORTRAIT_nour_nourneutral
    
    NOUR: "Do you know where you last saw them all?"
    
    &PORTRAIT_emperor evercrisp_emperorevercrispneutral
    
    EMPEROR EVERCRISP: "Their last coordinates were all within the hallways of the mall."
    
    &PORTRAIT_emperor evercrisp_emperorevercrisppreach
    
    EMPEROR EVERCRISP: "I will give you the sacred APLE BLESSING."
    
    &PORTRAIT_emperor evercrisp_emperorevercrispsmile
    
    EMPEROR EVERCRISP: "The blessing will allow you to teleport all <i>five</i> apples back to me."
    
    &PORTRAIT_emperor evercrisp_emperorevercrispneutral
    
    EMPEROR EVERCRISP: "Once they have consented, they shall disappear and return."
    
        &PORTRAIT_nour_nourneutral
    
    NOUR: "That sounds simple enough."
    
    &PORTRAIT_emperor evercrisp_emperorevercrispsmile
    
    EMPEROR EVERCRISP: "HO HO! EXCELLENT! I wish you luck, my human friends."
    
    &SHLORP_poop_EmperorEvercrisp_out
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Got it. Four apples, one Prince, all in the hallway."
    
    &EMOTE_NOUR_DownIdle
    
        &PORTRAIT_nour_noursmile
    
    NOUR: "Ready for an adventure, Nick?"
    
    &PORTRAIT_nick_nickneutral
    
    &EMOTE_NICK_DownIdle
    
    NICK: "..."
    
    Nick shrugs, but he's already itching to leave.
    
        &PORTRAIT_nour_nourneutral
    
    NOUR: <i>Still silent, but at least he's communicating again.</i>
        
    //check - every apple found, emperor tells how many are left
    
    //EMPEROR EVERCRISP: "If you require a count of how many of my Aple council brethren you have located, come back to consult me!"
    
    //EMPEROR EVERCRISP: "I shall tell you your APLE PROGRESS!"
    
        &PORTRAIT_nour_noursmile
    
    NOUR: "Sounds good. Guess we should head out to the hallway and start looking."
    
    &EMOTE_SUWAN_DownIdle
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "Do we really—"
    
    Ms. Suwan looks at Nick... and then she sighs.
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "Very well. Let's go find these... apples."
    
    &EMOTE_NOUR_LeftIdle
    
        &PORTRAIT_nour_noursmile
    
    NOUR: "Never thought I'd hear something like that from you."
    
    &PAUSE_1.0
    
    &AUDIO_FADEOUT
    
    &PAUSE_1.0
    
    &MUSIC_TopicSpotMusic
    
    &FOLLOW_NOUR_stop
    
    &FOLLOW_NICK_NOUR
    
    &FOLLOW_SUWAN_NICK
    
        -> END

== nourplaysaplequest ==

    &FOLLOW_NOUR_NourConsole
    
    &FOLLOW_SUWAN_SuwanConsole
    
    &FOLLOW_NICK_NickGoesAway
    
    &PAUSE_1.0
    
    &EMOTE_SUWAN_UpIdle
    
    &EMOTE_NOUR_UpIdle
    
    &EMOTE_NICK_UpIdle

    &PORTRAIT_nour_noursmile
    
    &EMOTE_NOUR_DownIdle

    NOUR: "Are you ready for the Secret Ending?"

    &PORTRAIT_nick_nickneutral
    
    NICK: "... Dunno."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Hmm. Not a lot of enthusiasm right there."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "..."
    
    &EMOTE_NOUR_UpIdle
    
    &PAUSE_0.1
    
    &EMOTE_NOUR_Gaming
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "Anway, here goes nothing..."
    
    &PAUSE_1.0
    
    &AUDIO_FADEOUT
    
    A tiny character pops up in the middle of the screen.
    
    Rows of weapons and a million character stats overwhelm the space. 
    
    &PAUSE_1.0

    &MUSIC_ApleQuestMusic
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "..."
    
    &PORTRAIT_nick_nickneutral
    
    NICK: "..."
    
    &EMOTE_NOUR_DownIdle
    
    &PORTRAIT_nour_nourpondering
    
        * "Have something to say?"
        
        * "Having fun watching?" 
        
        * "Want to join me?"
            
            &PORTRAIT_nick_nicknervous
    
            NICK: "... It's alright. It's a single player game, after all."
            
            &PORTRAIT_nour_nourpondering
            
            NOUR: "You sure? We could take turns."
                
            &PORTRAIT_nick_nicksad
            
            NICK: "It's fine."
            
            &PORTRAIT_nick_nicknervous
            
            NICK: "But uh..."
    
    - &PORTRAIT_nick_nicksad
    
    NICK: "Well, uh..."
    
    &FOLLOW_NICK_NickStepAway
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "Can I... can I watch?"
    
        &PORTRAIT_nour_noursmile
    
    NOUR: "Go ahead."
    
        &PORTRAIT_nick_nickshocked

    NICK: "..."
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "Um. Thanks."
    
    NOUR: "Sure."
    
    &PORTRAIT_nour_nourneutral
    
    &EMOTE_NOUR_UpIdle
    
    &PAUSE_0.1
    
    &EMOTE_NOUR_Gaming
    
    NOUR: "Okay, making your character... Shouldn't be too hard."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Hmm..."
    
        &PORTRAIT_nick_nicknervous
    
    NICK: "Are you sure you want to use a bow with a strength character?"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "It's not really an optimal build."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "You won't get a lot of use out of your character's strength..."
    
    &PORTRAIT_nick_nickneutral
    
    NICK: "Unless you use, like, a sword or something."
    
        &PORTRAIT_nour_nournervous
    
    NOUR: "I didn't know... I guess I'll switch over to another weapon."
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "Thanks for your help, Nick."
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "..."
    
        &PORTRAIT_nour_nourrealization
        
    NOUR: "Oh... but I like the bow. It looks nice."

    &PORTRAIT_nick_nicknervous
            
    NICK: "Oh, okay. You can try and change your stats a bit later..."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "Or you could just restart with a new character to match your style?"
    
    &PORTRAIT_nick_nicksmallsmile
            
    NICK: "If you like the bow, you should have a good time playing it from the beginning."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "... Or else, this run might really suck."
    
        &PORTRAIT_nour_noursmile
            
    NOUR: "... I think I'll do that, then."
    
    Nick focuses on the game, his leg bouncing up and down.
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "Let's go! First level should be a piece of cake."
    
    &PORTRAIT_nick_nickneutral
    
    NICK: "..."
    
    &PORTRAIT_nour_noursmile
    
    NOUR: <i>He's really getting into this.</i>
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>Why did he act like he hated this game before, anyway?</i>
    
            &PORTRAIT_nick_nickshocked
    
    NICK: "WATCH OUT!" 
    
        &PORTRAIT_nour_nourrealization
    
    NOUR: "SHOOT!"
    
    The character barely dodges a hidden enemy. 
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "... <i>NICK</i>, you SCARED me."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "Crap, sorry..."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "You almost stepped into a trap that would've insta-killed you!"
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "You see those red glowing orbs? Fire trap. It <i>blows</i>... <i>literally</i>."
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "Haha!"
    
        &PORTRAIT_nour_noursmile
    
    NOUR: "Nice pun."
    
    &PORTRAIT_ms. suwan_mssuwansmile
    
    MS. SUWAN: "Heh."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "No way... Was that a... ?"
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "Ms. Suwan..."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "You know how to <i>laugh</i>?"
    
    &EMOTE_SUWAN_DownIdle
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "<i>Yes!</i>"
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "... I'm not <i>that</i> heartless."
    
    &PORTRAIT_ms. suwan_mssuwansmile
    
    MS. SUWAN: "And I am particularly weak to <i>good</i> word play."
    
    &EMOTE_NOUR_DownIdle
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "Nick... You've achieved the greatest feat of all..."
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "Making Ms. Suwan <i>laugh</i>!"
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "..."
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "You know, maybe I should stop playing." 
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "It's probably not fun to watch me die every five seconds."
    
            &PORTRAIT_nick_nicknervous
    
    NICK: "No! It's fine. This is technically not <i>supposed</i> to be an easy game..."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "I should just shut up and let you play."
    
    &PORTRAIT_nick_nickshadow
    
    NICK: "It's not like I'm being helpful by shouting in your ear."
    
    &PORTRAIT_nour_noursad
    
        *"But you seem really knowledgeable about the game."
        
            &PORTRAIT_nour_noursmile
        
            NOUR: "You know that's actually a good thing for a noob like me, right?"
            
                &PORTRAIT_nick_nicksmallsmile
            
            NICK: "I might know a thing or two..."
        
            &PORTRAIT_nick_nickbigsmile
            
            NICK: "I <i>am</i> a mega Aple-head, after all."
            
        *"No, don't shut up! I'm only surviving because of you!" 
            
            &PORTRAIT_nour_nourpondering
            
            NOUR: "If you don't keep telling me all your tips, I'm gonna die way more."
            
            &PORTRAIT_nour_noursmile
            
            NOUR: "That would be the worst for a noob like me! "
            
            &PORTRAIT_nick_nicksmallsmile
            
            NICK: "That would be pretty bad..."
            
    - &PORTRAIT_nick_nickneutral
    
    NICK: "You know, even though <i>Aple Quest</i> is my favorite game..."
    
    &PORTRAIT_nick_nickangry
    
    NICK: "The only problem is that the tutorial is kind of garbage."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "It assumes the player already knows a bunch of stuff, which isn't true."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "... Hey! You know what I've been thinking?
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "You can totally make gaming your fun hobby."
    
    &PAUSE_1.0
    
    &AUDIO_FADEOUT
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "I-I can't..."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "I watch gaming videos and watch <i>others</i> play it, but..."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "Don't you need to <i>do</i> the hobby yourself to call it your hobby?"
    
    &PORTRAIT_nick_nickshadow
    
    NICK: "Because I don't... actually own the game."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "I've only played it because your brother has it."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "Calling gaming my hobby when I don't even <i>have</i> a copy of my favorite game?"
    
    &PORTRAIT_nick_nickshadow
    
    NICK: "... What a joke."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "..."
    
    &PORTRAIT_nick_nickangry
    
    NICK: "Plus, it's such a useless hobby. That's what my Mom says."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "That's what <i>everybody</i> says."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "..."
    
    &PORTRAIT_nick_nickshadow
    
    NICK: "I'd just be a dumb kid with dumb interests and no future."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "And... And... !"
    
    &PORTRAIT_nour_nourneutral
    
        * "..."
        
        * "It's alright, you can talk to me about it..."
    
    - &PORTRAIT_nick_nicknervous
    
    NICK: "..."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "My mom wouldn't let me get the game because my grades were crap." 
    
    &PORTRAIT_nick_nickshadow
    
    NICK: "She said I could get it if I fixed them, but it's..."
    
    &PORTRAIT_nick_nicktears
    
    NICK: "It's really <i>hard</i>. I'm too dumb to do anything right." 
    
    &PAUSE_1.0

    &MUSIC_ApleQuestEmoMusic
    
    &PORTRAIT_nour_noursad
    
    NOUR: "You're not dumb, Nick."
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "You know so much about this game, probably far more than you've let on."
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "You know that makes you smart, right?"
    
    &PORTRAIT_nick_nickangry
    
    NICK: "But it's not the <i>same</i>!"
    
    &PORTRAIT_nick_nicksad
    
    NICK: "It's a dumb video game... not <i>math</i> or <i>science</i> or stuff like that."
    
    &PORTRAIT_nick_nicktears
    
    NICK: "I don't have the time to focus on school.
    
    &PORTRAIT_nick_nickshadow
    
    NICK: "Even when I <i>do</i> it's so <i>hard</i>."
    
    &PORTRAIT_nick_nicktears
    
    NICK: "It's like there's a... a brick wall in front of <i>everything</i>."
    
    &PORTRAIT_nour_nourneutral
    
        * "But you love this game. You've dedicated so much time to it."
            
            &PORTRAIT_nour_nournervous
            
            NOUR: "You're not useless or dumb for liking something a lot."
            
            &PORTRAIT_nour_nourneutral
            
            NOUR: "Memorizing everything about anything takes a good amount of smarts."
            
            &PORTRAIT_nick_nickshocked
            
            NICK: "You mean that?"
            
                &PORTRAIT_nour_noursmile
            
            NOUR: "Yeah, I do."
            
            &PORTRAIT_nick_nicksad
            
            NICK: "No one's ever..."
            
            &PORTRAIT_nick_nicktears
            
            NICK: "<i>Everyone</i> thinks I'm dumb. I've been useless my entire life..."
            
            &PORTRAIT_nick_nicksad
            
            NICK: "But yeah... I love this game. It means a lot to me."
            
            NICK: "I've always wanted to dedicate more time to it."
            
            &PORTRAIT_nick_nicktears
            
            NICK: "But it's not that easy!"
        
        * "I'm sorry that it's been so hard for you... but acting out isn't going to make it better."
        
            &PORTRAIT_nour_noursad
        
            NOUR: "If you keep doing this, then you'll never be able to truly get into the thing you love."
        
            &PORTRAIT_nick_nicksad
        
            NICK: "So what should I do?"
    
    - &PORTRAIT_nick_nickshadow
            
    NICK: "My life is a <i>mess</i>. I don't know what else to do."
    
    Nick looks at the bracelet.
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "..."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "And I'm scared..."
    
    &PORTRAIT_nick_nicktears
    
    NICK: "... because I know <i>I'm</i> the reason why my life's a mess."
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "Well, your self-awareness is a good start."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "... !"
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Rebeling and getting yourself into trouble?"
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "Those won't help you regain control over your life."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "You'll be chasing a temporary feeling for a long time with those ways..."
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "Agency comes with confronting yourself and your responsibilities."
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "That, and confidence! Because you <i>can</i> handle what life throws at you."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "You should focus as much as you can on school."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "If you do well—"
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "And I know you will!"
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "Then, you can dedicate the rest of your life to games..."
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "Playing them, making them, whatever you want."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "Whatever you do, <i>never</i> feel like you aren't smart enough for it."
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "Because you <i>are</i>."
    
    NOUR: "Right, Ms. Suwan?"
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "..."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "Intelligence comes in many forms."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "Society may value one form above the rest..."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "... but it does not make the others less valuable."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "I... am guilty of doing this."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "I looked down on your passions—what made you the happiest."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "But I was wrong."
    
    &PORTRAIT_ms. suwan_mssuwansmile
    
    MS. SUWAN: "Your knowledge has helped us so much."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "... Far more than any ounce of knowledge I have, degree be damned."
    
    &PORTRAIT_ms. suwan_mssuwansmile
    
    MS. SUWAN: "You have proven that you are resourceful, bright, and compassionate."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "And you've done it over and over again. Especially with those apples."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "I'm ashamed of my behavior. I should know better, and I..."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "...I'm so sorry, Nick. You are smart and very capable."
    
    &PORTRAIT_ms. suwan_mssuwansmile
    
    MS. SUWAN: "Don't let people like me ever tell you otherwise."
    
    &PORTRAIT_nick_nicktears
    
    NICK: "..."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "Oh. I... Hold on, Nick..."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "I have tissues somewhere..."
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "..."
    
    He slowly slides the bracelet off, holding it out to Nour. 
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "... Here. Can you put it back for me?"
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "I... I don't need it anymore."
    
    &PORTRAIT_nour_noursmile
    
    Nour smiles and takes the bracelet.
    
    NOUR: "Sure thing, Nick."
    
    &PORTRAIT_nick_nickbigsmile
    
    &FOLLOW_NICK_NickConsole
    
    &FOLLOW_SUWAN_SuwanLookAtNick
    
    &FOLLOW_NOUR_NourLookAtNick
    
    &PAUSE_1.0
    
    &EMOTE_NOUR_UpIdle
    
    &EMOTE_SUWAN_UpIdle
    
    &EMOTE_NICK_UpIdle
    
    NICK: "Okay, now back to the game."
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "If I speedrun this thing fast enough..."
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "I bet I could reach that Secret Ending!"
    
    &MOV_RINA_TOPICSPOT
    
        &PORTRAIT_nick_nickshocked
        
    &PAUSE_0.5
    
    &EMOTE_NICK_RightIdle
    
    &EMOTE_NOUR_RightIdle
    
    &EMOTE_SUWAN_RightIdle
    
    NICK: "AGH, she's here! What does she want now?"
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "Uh-"
    
        &PORTRAIT_nick_nickpondering
    
    NICK: "You know what, Nour..."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "It's my turn to give <i>you</i> some advice!"
    
    &EMOTE_NOUR_LeftIdle
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "I think you should confront her about how she treated you."
    
        &PORTRAIT_nick_nickshocked
    
    NICK: "You can do it!"
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>... I guess I should talk to her.</i>
    
    &PAUSE_1.0
    
    &MUSIC_TopicSpotMusic
    
    &FOLLOW_NOUR_stop
    
    &FOLLOW_NICK_NOUR
    
    &FOLLOW_SUWAN_NICK
    
    &LIGHT_Nick2

    -> light_on

=== consoles_afternickroute ===

    &FOLLOW_NICK_NickConsole
    
    &FOLLOW_SUWAN_SuwanConsole
    
    &FOLLOW_NOUR_NourConsole
    
    &PAUSE_1.0
    
    &EMOTE_NOUR_UpIdle
    
    &EMOTE_NICK_UpIdle
    
    &EMOTE_SUWAN_UpIdle

    The bow-user character from the last game played stands idle on the screen.
    
        &PORTRAIT_nick_nickbigsmile
    
    NICK: "Next time, we should try to play a tank character!"
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "Defense is the best offense, right?"
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "It's the opposite, actually."
    
        &PORTRAIT_nick_nicknervous
    
    NICK: "Oh..."
    
    &PORTRAIT_ms. suwan_mssuwansmile
    
    MS. SUWAN: "But I'm not the expert, remember? That's you."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "So perhaps, defense <i>is</i> the best offense."
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "Maybe! I guess the only way to find out would be to play..."
    
    &FOLLOW_NOUR_stop
    
    &FOLLOW_NICK_NOUR
    
    &FOLLOW_SUWAN_NICK
    
        -> END