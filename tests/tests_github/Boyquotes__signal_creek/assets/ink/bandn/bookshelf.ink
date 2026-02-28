== bookshelf ==

    It's a tall bookshelf filled with many informative books.

    Instructional guides, exam prep workbooks, and how-to books for those interested in <i>fun</i> new hobbies!
    
    &FOLLOW_NOUR_stop
    
    {
        // - noursuggestion && nicktalksaboutfantasygame:
            // -> END
        - bookshelf_afternickroute:
            -> bookshelf_end
            
        - aplequestguide && nourplaysaplequest:
            -> bookshelf_afternickroute
            
        - aplequestguide_after_bookshelf_fightstarts:
            -> bookshelf_afterfight
            
        - nour_suggests_hobbies && consoles:
            -> bookshelf_nour_suggests_hobbies_repeat
            
        - spraycans_start:
            -> nour_suggests_hobbies
    }
    
    -> END
    
== nour_suggests_hobbies ==


    &FOLLOW_NICK_NickBookshelfPos
    &FOLLOW_SUWAN_SuwanBookshelfPos
    &FOLLOW_NOUR_NourBookshelfPos
    &PAUSE_1
    &EMOTE_SUWAN_RightIdle
    &EMOTE_NICK_UpIdle
    &EMOTE_NOUR_LeftIdle

    &PORTRAIT_nour_nourneutral

    NOUR: "...This makes me think about high school."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "My parents expected me to get the best possible exam scores."
        
    &PORTRAIT_nick_nicksad
        
    NICK: "I don't really care about tests."
        
    &PORTRAIT_nour_nournervous
        
    NOUR: "Really?"
        
    &PORTRAIT_nick_nickangry
        
    NICK: "I mean, yeah. Who even cares if I know PEMDAS or the Pythagorean Theorem?"
        
    &PORTRAIT_ms. suwan_mssuwanangry
        
    MS. SUWAN: "Your college admissions counselors."
        
    &PORTRAIT_nick_nicksad
        
    NICK: "I'm not even thinking about college right now. I'm only sixteen."
        
    &PORTRAIT_ms. suwan_mssuwanneutral
        
    MS. SUWAN: "Sixteen is the time to start applying yourself more."
        
    &PORTRAIT_ms. suwan_mssuwanpondering
        
    MS. SUWAN: "If you want to get into a good college, that is."
        
    &PORTRAIT_nick_nickneutral
        
    NICK: "Nah. I just wanna have fun while I can, y'know?"
        
    &PORTRAIT_ms. suwan_mssuwanangry
        
    MS. SUWAN: "If you have fun now, you <i>will</i> struggle later."
        
    &PORTRAIT_nick_nickangry
        
    NICK: "But I don't want to live that way."
        
    &PORTRAIT_ms. suwan_mssuwanannoyed
        
    MS. SUWAN: "Well, we aren't often given a choice."
        
    &PORTRAIT_nour_nournervous

    NOUR: <i>Things are getting a little bit heated. I should try to change the subject.</i>
        
    &PORTRAIT_nour_noursmile
    
        * "Look at this, Nick... <i>How to Draw Manga Characters</i>!"
            -> howtodrawmangachara
        
        * "Hey... there's <i>The Making of Aple Quest - The Classic RPG Series</i>!"
            -> howtomakeaplequest
        
        * "There's also... <i>Intro to Persusasive Arguments</i>."
        
            -> howtomakearguments
                
=== howtodrawmangachara ===

    NOUR: "This would be a pretty fun hobby, right?"
            
        * "Are you interested?"
                
            &PORTRAIT_nour_nourrealization
                
            NOUR: "It might be cool to try learning something new."
                    
            &PORTRAIT_nour_noursmile
                    
            NOUR: "That's what got me into photography."
                    

        * "People who can draw are pretty cool."
                
            &PORTRAIT_nour_nourrealization
                
            NOUR: "You might end up liking it."
                    
            &PORTRAIT_nour_noursmile
                    
            NOUR: "That was my experience with photography."
        
    - &PORTRAIT_nick_nicksad
                
    NICK: "I don't know. I'm not really good at art and stuff."
                
    &PORTRAIT_ms. suwan_mssuwanangry
            
    MS. SUWAN: "I think Nick should focus on finding a hobby that would benefit his future."
                
    &PORTRAIT_nour_nourpondering
                
    NOUR: "Well, a hobby is something you <i>enjoy</i>, not necessarily—"
                
    &PORTRAIT_nick_nickangry
                
    NICK: "This hobby needs talent <i>and</i> doesn't benefit my future?"
                
    &PORTRAIT_nick_nicksad
                
    NICK: "Not interested."
    
    -> nour_suggests_hobbies_cont   
    
=== howtomakeaplequest ===

    &PORTRAIT_nour_nourpondering
            
    NOUR: "Isn't this the game you wanted to see before?"
            
    NOUR: "The one in TopicSpot's game console?"
            
    &PORTRAIT_nour_nourrealization
            
    NOUR: "This could be an interesting hobby."
            
    &PORTRAIT_nour_noursmile
            
    NOUR: "You could become a game designer!"
            
    &PORTRAIT_nick_nicknervous
            
    NICK: "It's not that easy. I don't know how to code."
            
    &PORTRAIT_nour_nourpondering
            
    NOUR: "You could learn to be an expert at this game, then."
            
    &PORTRAIT_nour_noursmile
            
    NOUR: "It could be like studying history, except it's something you like."
            
    &PORTRAIT_nick_nicksad
            
    NICK: "I don't want to study something that I like. That would ruin it."
            
    &PORTRAIT_nick_nickshadow
            
    NICK: "...And I'd be no good at memorizing stuff like that. I'm not that smart."
    
        -> nour_suggests_hobbies_cont
        
=== howtomakearguments ===

    &PORTRAIT_nour_nourpondering
        
    NOUR: "Seems like a pre-pre-LSAT kind of book."
            
    &PORTRAIT_nour_noursmile
            
    NOUR: "Good for beginners looking to learn more!"
            
    &PORTRAIT_nick_nickangry
            
    NICK: "I think I'd rather die than read something like that."
            
    &PORTRAIT_ms. suwan_mssuwanneutral
        
    MS. SUWAN: "It would be best for you to remain open-minded."
            
    &PORTRAIT_ms. suwan_mssuwanpondering
            
    MS. SUWAN: "I had no idea I would enjoy law until I came across good study aids."
            
    &PORTRAIT_ms. suwan_mssuwanneutral
            
    MS. SUWAN: "Taking that extra step ensures that you excel at what you do."
            
    &PORTRAIT_ms. suwan_mssuwanpondering
            
    MS. SUWAN: "This could be something."
            
    &PORTRAIT_nick_nicksad
            
    NICK: "I don't wanna force myself to like something. That's not how you pick up interests."
            
    &PORTRAIT_nick_nickshadow
            
    NICK: "...And either way, I wouldn't understand it, no matter how hard I tried."
    
        -> nour_suggests_hobbies_cont
    
=== nour_suggests_hobbies_cont ===

    &PORTRAIT_nour_nournervous
    
    NOUR: "What about..."
    
    &PORTRAIT_nour_nourpondering
    
        * "Art History Around the World?"
        
        * "Mechanical Engineering 101?"
        
        * "JavaScript for Beginners?"
        
    - &PORTRAIT_nick_nickangry
    
    &EMOTE_NICK_Jump
    
    NICK: "I literally <i>don't care</i>. I don't want to study anything."
    
    &EMOTE_NICK_UpIdle
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "You won't not get far in life if you cannot change your mindset."
    
    &PORTRAIT_nour_nourneutral
    
        * "That's a bit harsh, Ms. Suwan..."
        
            &PORTRAIT_ms. suwan_mssuwanangry
        
            MS. SUWAN: "Listen, if this kid's parents failed to give him the truth, someone has to."
            
            &PORTRAIT_ms. suwan_mssuwansad
            
            MS. SUWAN: "I had to learn the hard way."
        
        * "Nick, people will think you're more capable if you know how to do something."
        
            &PORTRAIT_nour_noursmile
        
            NOUR: "Isn't that what you want?"
            
            &PORTRAIT_nick_nicksad
        
            NICK: "I don't have to know how to do something that requires me to <i>read</i> for fun."
            
            &PORTRAIT_nick_nickshadow
            
            NICK: "I just... I don't know." 
            
            NICK: "It feels like a waste of time."
            
            &PORTRAIT_ms. suwan_mssuwanangry
            
            MS. SUWAN: "As if you're not wasting your time now, being a delinquent teenager?"
        
    - &PORTRAIT_nick_nickangry
    
    NICK: "You're old and boring. Study <i>this</i>, responsiblility <i>that</i>—whatever."
    
    NICK: "What happened to having a <i>life</i>?"
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "That was incredibly rude."
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "I <i>have</i> a life."
    
    MS. SUWAN: "One I worked tooth and nail to earn."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "...While others got things handed to them on a silver plate."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "It is a <i>privilege</i> to get the opportunity to learn."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "Especially without constantly worrying about factors outside of your control."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "I just don't see why I need to waste all of my time."
    
    &PORTRAIT_nick_nickshadow
    
    NICK: "And I'm not being a <i>delinquent teenager</i>."
     
    NICK: "I wanna have fun before I gotta start worrying about adult problems."
    
    &PORTRAIT_nour_noursad
    
        * "Making the real world seem bleak won't help anyone."
        
            &PORTRAIT_nour_noursmile
        
            NOUR: "You can have fun practicing your skills."
            
            &PORTRAIT_nour_nourpondering
            
            NOUR: "It may even lead you somewhere in life."
        
        * "You can have fun and work hard at the same time..."
        
            &PORTRAIT_nour_noursad
        
            NOUR: "Not everything has to be miserable. Not even work."
    
    - &PORTRAIT_nour_nourneutral
    
    NOUR: "Look, whether I make money from photography or not, I'll always love doing it."
    
    &PORTRAIT_nick_nickangry
    
    NICK: "Aren't <i>you</i> lucky? You have something you're good at."
    
    NICK: "Well, guess what, Nour? Not everyone is smart or good at something."
    
    &PORTRAIT_nour_noursad
    
        * "I wasn't born with the ability to take pictures."
            
            &PORTRAIT_nour_nourneutral
            
            NOUR: "You can only get better at something through practice."
        
        * "How do you know I'm good at it?"
        
            &PORTRAIT_nour_nourneutral
        
            NOUR: "For all you know, I could suck at taking pictures."
            
            &PORTRAIT_nour_noursad
        
            NOUR: "The time I've put into what you think I'm 'naturally' good at isn't visible."
            
    - &PORTRAIT_nick_nicksad
    
    NICK: "Forget it. Neither of you guys would get it."
    
    &PORTRAIT_nick_nickangry
    
    NICK: "Some of us are either born dumb, or end up liking useless crap like <i>Aple Quest</i>."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "...Or both."
    
    &PORTRAIT_nick_nickshadow
    
    NICK: "I could never be like you two. Not now, and not ever."
    
    &NICK_EMOTE_DownIdle
    
    &PORTRAIT_nour_noursad

    NOUR: <i>He sounds so upset. I've never seen him like this before.</i>
    
    NOUR: <i>It doesn't look like he's open to learning anything new right now.</i>
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>But I still want to cheer him up.</i>
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: <i>Maybe I've been approaching this all wrong.</i>
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>Maybe I could find something in this store that already makes him happy...</i>
    
    &FOLLOW_NOUR_stop
        
    -> END
    
=== bookshelf_nour_suggests_hobbies_repeat ===

    &PORTRAIT_nour_nourpondering

    NOUR: <i>Talking about learning and studying made Nick more upset than anything.</i>
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>I don't know why quite yet, but I want to make him feel better.<i>
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: <i>Maybe something less... studious.</i>
    
    NOUR: <i>Like that videogame from earlier he wanted to see.</i>
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>What was it called again? It was fruit related... like Orange Odyssey?</i>
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>I'm sure if I look around this bookstore, I'm bound to find something related to it.</i>
    
    &FOLLOW_NOUR_stop
        -> END
    
=== bookshelf_afterfight ===

    &PORTRAIT_nour_nourneutral

    NOUR: <i>I don't think I need to see anything else here.</i>
    
    -> END
    
=== bookshelf_afternickroute ===

    &FOLLOW_NICK_NickBookshelfPos
    &FOLLOW_SUWAN_SuwanBookshelfPos
    &FOLLOW_NOUR_NourBookshelfPos
    &PAUSE_1
    &EMOTE_SUWAN_RightIdle
    &EMOTE_NICK_UpIdle
    &EMOTE_NOUR_LeftIdle

    &PORTRAIT_nick_nickneutral

    NICK: "Were you always good at lawyer-ing, Ms. Suwan?"
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "<i>Arguing</i>, yes. Law, on the other hand, no. I had to start from the bottom."
    
    &PORTRAIT_ms. suwan_pondering
    
    MS. SUWAN: "I was still learning English while I was becoming a lawyer."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "Isn't there, like, a lot of reading and writing though? Wasn't that hard?"
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "Yes. Every failure told me to give up, but then I would remember how far I had come."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "It's easy to lose track of that when you focus on the future."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "How far I've come, huh... I never thought of it that way."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "Nick, you're still very young. You know what you like, and that's good."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "It would be a shame if we lost an important engineer or manga artist."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "Especially if it's because he gave up before he even tried."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "Because he didn't <i>believe</i> in himself."
    
    NICK: "..."
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "...Thanks, Ms. Suwan."
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "Also, do you know what manga is?"
    
    &PORTRAIT_ms. suwan_mssuwansmile
    
    MS. SUWAN: "Of course I know <i>what<i> it is. But I don't consume it."
    
    MS. SUWAN: "I would be happy to learn more about it."
    
    &EMOTE_NICK_Jump
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "I'd <i>love</i> to tell you about it!" 
    
    NICK: "I have so many favorite series..."

    -> bookshelf_end

=== bookshelf_end ===

    &PORTRAIT_nour_nourneutral

    NOUR: <i>I think we've explored everything we could here.</i>
    
    &FOLLOW_NOUR_stop
        
        -> END