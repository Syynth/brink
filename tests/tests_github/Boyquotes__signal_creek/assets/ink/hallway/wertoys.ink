== wertoys ==

    It's the old toy store, <i>We R Toys</i>.

    {
        - wertoys_find_prince:
            -> wertoys_end
            
        - consoles_start_aplequest && beautystand_find_pinklady && bobatea_find_dukedelicious && fountain_findfather && weirdtree_findgranny:
            -> wertoys_find_prince
            
        - wertoys_before_aplequest:
            -> wertoys_before_aplequest_repeat
            
        - else:
            -> wertoys_before_aplequest
    }
    

== wertoys_before_aplequest ==

    &PORTRAIT_ms. suwan_mssuwanannoyed

    MS. SUWAN: "It's been ransacked. Typical."
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "Those who break into this mall are <i>children</i>."
    
    &PORTRAIT_nour_noursad

    NOUR: "I remember fighting with my parents here...
    
    &PORTRAIT_nick_nicksad
    
    NICK: "About what?"
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "...About what kind of toys are normal for girls."
    
    &PORTRAIT_nick_nickangry
    
    NICK: "Man, talk about a mood killer."
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "Like, what did the toy store do to you two?"
    
    -> END
    
== wertoys_end ==
    
    After an exchange of encouraging words, the Prince left the store.
    
    Perhaps, the Aple Prince left Earth...
    
    -> END

== wertoys_find_prince == 

    &PAUSE_1.0
    
    &AUDIO_FADEOUT

    NOUR: "..."

    PORTRAIT_nour_nourpondering
    
    &FOLLOW_NICK_NickToyPos
    &FOLLOW_SUWAN_SuwanToyPos
    &FOLLOW_NOUR_NourToyPos
    
    &PAUSE_1.0
    
    &EMOTE_NICK_UpIdle
    &EMOTE_SUWAN_UpIdle
    &EMOTE_NOUR_UpIdle
    
    There's a small groaning sound coming from below the gate.
    
    NOUR: "..."
    
    MS. SUWAN: "What... is that <i>noise</i>?"
    
    &PORTRAIT_???_princependragonsad
    
    ???: "I feel... so... useless..."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "What the..." 
    
    NOUR: "Hello?"
    
    &SHLORP_APPLE_PrincePendragon_in
    
    ???: "Down here."
    
    &EMOTE_NOUR_Jump
    &EMOTE_NICK_Jump
    &EMOTE_SUWAN_Jump
    
    &PAUSE_0.5
    
    &EMOTE_NICK_UpIdle
    &EMOTE_SUWAN_UpIdle
    &EMOTE_NOUR_UpIdle
    
    &PORTRAIT_prince pendragon_princependragonneutral
    
    PRINCE PENDRAGON: "I am the fallen prince... <i>Prince Pendragon</i>."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "...Uh-"
    
    &PORTRAIT_prince pendragon_princependragontears
    
    PRINCE PENDRAGON: "<i>FALLEN</i>, I say!"
    
    &PORTRAIT_prince pendragon_princependragonsad
    
    PRINCE PENDRAGON: "Fallen from the Mother Tree, fallen from the sky, fallen from grace..."
    
    &PORTRAIT_prince pendragon_princependragontears
    
    PRINCE PENDRAGON: "...onto this floor."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "You're <i>the</i> Prince?"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "...!"
    
    &PORTRAIT_prince pendragon_princependragonbashful
    
    &SOUND_princependragonlaugh
    
    PRINCE PENDRAGON: "You've heard of little ol' me?"
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "We've heard many things about you from the council."
    
    &PORTRAIT_prince pendragon_princependragonsad
    
    PRINCE PENDRAGON: "The others speak of nothing but their disapointment in me, the <i>failure</i>, I presume?"
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Actually, the opposite."
    
    &PORTRAIT_nour_nourneutral
    
    &PORTRAIT_prince pendragon_princependragonneutral
    
    * "They said your people need you."
    
        PRINCE PENDRAGON: "They need... <i>me</i>?
    
    * "They said you have a noble heart."
    
        PRINCE PENDRAGON: "Me? A noble heart?"
        
    * "They said you are beloved by all."
    
        PRINCE PENDRAGON: "I am... beloved?"
        
    - &PORTRAIT_nour_nourrealization
    
    NOUR: "The Princess sends her regards as well!"
    
    &PORTRAIT_prince pendragon_princependragonbashful
    
    PRINCE PENDRAGON: "Princess Pink Lady... !"
    
    &PORTRAIT_prince pendragon_princependragonsad   
    
    PRINCE PENDRAGON: "But... but no! That cannot be true."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "Strangely enough, I am <i>somehow</i> able to corroborate."
    
    &PORTRAIT_prince pendragon_princependragontears
    
    PRINCE PENDRAGON: "You do not understand!"
    
    &PORTRAIT_prince pendragon_princependragonsad
    
    PRINCE PENDRAGON: "Daddy threw me away, like the rotten little boy I am."
    
    &PORTRAIT_prince pendragon_princependragonneutral
    
    PRINCE PENDRAGON: "He sent me to Earth as punishment for my failures on the battlefield."
    
    &PORTRAIT_prince pendragon_princependragonsad
    
    PRINCE PENDRAGON: "The floor is all I know, and it is where I belong."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "You've gotta pick yourself up off that floor at some point."
    
    &PORTRAIT_prince pendragon_princependragonneutral
    
    PRINCE PENDRAGON: "How can I? My skin is not firm enough to roll me forward!"
    
    &PORTRAIT_prince pendragon_princependragonsad
    
    PRINCE PENDRAGON: "My flesh isn't juicy enough for the lady apples!"
    
    &PORTRAIT_prince pendragon_princependragontears
    
    PRINCE PENDRAGON: "And I am too <i>tiny</i> for the crown!"
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Uh... What?"
    
    &PORTRAIT_prince pendragon_princependragonsad
    
    PRINCE PENDRAGON: "I'll never be brave, smart, or juicy enough."
    
    &PORTRAIT_prince pendragon_princependragontears
    
    PRINCE PENDRAGON: I am simply unfit to be a prince."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "..."
    
    &PORTRAIT_nour_noursad
    
    &PORTRAIT_prince pendragon_princependragonsad
    
    * "There's more to apples than juice and skin."
    
        PRINCE PENDRAGON: "More than juice and skin? Nothing else makes a good apple."
    
    * "Why did the Emperor really send you here?"
    
        PRINCE PENDRAGON: "Like I told you... I am not fit to be prince."
    
    - &PORTRAIT_nour_nourrealization
    
    NOUR: "I'd have to disagree."
    
    &PORTRAIT_prince pendragon_princependragonneutral
    
    PRINCE PENDRAGON: "How?"
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Well, for one... You seem like a sweet apple."
    
    &PORTRAIT_prince pendragon_princependragonbashful
    
    PRINCE PENDRAGON: "Hoo hoo... you think so?"
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "And nutritious..."
    
    &PORTRAIT_prince pendragon_princependragonneutral
    
    PRINCE PENDRAGON: "Human, I am not for consumption!"
    
    &PORTRAIT_prince pendragon_princependragonbashful
    
    PRINCE PENDRAGON: "But you flatter me..."
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "Just because you don't fit the mold of a typical apple..."
    
    &PORTRAIT_nick_nickneutral
    
    NICK: "..."
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "...doesn't mean you're any less worthy."
    
    &PORTRAIT_prince pendragon_princependragontears
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "..."
    
    * "Remind yourself of your strengths."
    
        NOUR: "You have a lot to offer to the world."
        
        PRINCE PENDRAGON: "What could I offer? I have nothing."
    
    * "Don't compare yourself to others."
        
        PRINCE PENDRAGON: "It is hard not to... There are far better apples than I..."
    
    - &PORTRAIT_nick_nicksad 
    
    NICK: "No, that's not true, Prince!"
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "...!"
    
    &PAUSE_1.0

    &MUSIC_ApleQuestEmoMusic
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "The only reason Emperor Evercrisp sent you to Earth was for your own protection."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "From the Fruit Wars!"
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "It wasn't about your worth."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "You're really smart, and good at picking up on things about people."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "Remember when you used your observation skills to figure out the Cornling's battle strategy?"
    
    &PORTRAIT_prince pendragon_princependragonneutral
    
    PRINCE PENDRAGON: "Ah, yes... I studied Cornlings for <i>months</i>..."
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "And you were able to help the Bean Boys get their town back from the Cornling army."
    
    &PORTRAIT_prince pendragon_princependragonsmile
    
    PRINCE PENDRAGON: "You know, I met the Prince of Corn... In fact, he and I became friends."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "No way!"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "So the secret traitor of the Cornlings..."
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "It was the Prince of Corn this WHOLE TIME?"
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "That wasn't even in <i>Aple Quest</i>! Oh man..."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "But that's besides the point!"
    
    &PORTRAIT_nick_nicksad
    
    NICK: "Prince Pendragon! You aren't a failure..."
    
    &PORTRAIT_prince pendragon_princependragontears
    
    PRINCE PENDRAGON: "..."
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "You're a <i>hero</i>. It takes a lot of smarts and guts to do all that!"
    
    &PORTRAIT_prince pendragon_princependragonsad
    
    PRINCE PENDRAGON: "...Perhaps focusing on my mistakes clouded my mind."
    
    &PORTRAIT_prince pendragon_princependragonsmile
    
    PRINCE PENDRAGON: "We are not our mistakes! We are not our failures!"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "That's right! We <i>aren't</i>! You shouldn't be the one to bring yourself down."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "And... I know what it's like to wanna give up on everything."
    
    &PORTRAIT_nick_nickshadow
    
    NICK: "Not feeling good enough for anyone."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "But the council, the Prince of Corn, and the Emperor... They care about you!"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "They need you to win the war, but they also wanted you to be safe!"
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "And now they're worried they can't find you!"
    
    &PORTRAIT_prince pendragon_princependragonsad
    
    PRINCE PENDRAGON: "I can't believe I did that to everyone..."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "They <i>love</i> you no matter what, Prince Pendragon!"
    
    &PORTRAIT_prince pendragon_princependragontears
    
    PRINCE PENDRAGON: "How could I have forgotten... the very reason I fight?"
    
    &PAUSE_1.0
    
    &AUDIO_FADEOUT
    
    &PORTRAIT_prince pendragon_princependragonneutral
    
    PRINCE PENDRAGON: "I can't afford to wallow in self-pity! The people need me."
    
    &PORTRAIT_prince pendragon_princependragonsad
    
    PRINCE PENDRAGON: "My... My <i>family</i> needs me!"
    
    &PAUSE_1.0

    &MUSIC_ApleQuestMusic
    
    &PORTRAIT_prince pendragon_princependragonneutral
    
    PRINCE PENDRAGON: "Thank you, human... I see the wisdom in my father's decision to send me here."
    
    &PORTRAIT_prince pendragon_princependragonsmile
    
    PRINCE PENDRAGON: "And likewise, his decision to send YOU all here to bring me back!"
    
    &PORTRAIT_prince pendragon_princependragonbashful
    
    PRINCE PENDRAGON: "Perhaps..."
    
    &PORTRAIT_prince pendragon_princependragonsmile
    
    PRINCE PENDRAGON: "It is <i>I</i> who will bring peace to the fruits!"
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "You can do it!"
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "As long as you remember the way of the Mother Tree..."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "<i>By the power of pulp and seed—</i>"
    
    &PORTRAIT_prince pendragon_princependragonsmile
    
    PRINCE PENDRAGON: "<i>I choose to believe in ME!</i>"
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "That's right! Now get your apple bottom to space and FIGHT ON!"
    
    &SOUND_princependragonlaugh
    
    PRINCE PENDRAGON: "HOO HOO! I can feel the juices FLOWING... !"
    
    &PORTRAIT_prince pendragon_princependragonneutral
    
    PRINCE PENDRAGON: "I feel as though my journey is just beginning..."
    
    &PORTRAIT_prince pendragon_princependragonbashful
    
    PRINCE PENDRAGON: "...but it will be different this time."
    
    &PORTRAIT_prince pendragon_princependragonneutral
    
    PRINCE PENDRAGON: "For the those who have always believed in me..."
    
    &PORTRAIT_prince pendragon_princependragonbashful
    
    PRINCE PENDRAGON: "For the friends I've made along the way!"
    
    &PORTRAIT_prince pendragon_princependragonsmile
    
    PRINCE PENDRAGON: "This time, perhaps, <i>things will end differently</i>!"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "Wait! You don't mean..."
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "A SECRET ENDING!?"
    
    &SOUND_princependragonlaugh
    
    PRINCE PENDRAGON: "HOO HOO! There's only one way to find out!"
    
    &PORTRAIT_prince pendragon_princependragonbashful
    
    PRINCE PENDRAGON: "I will never forget you, human! From my core, THANK YOU!"
    
    &PORTRAIT_prince pendragon_princependragonneutral
    
    PRINCE PENDRAGON: "And from one Prince to another..."
    
    &PORTRAIT_prince pendragon_princependragonsmile
    
    PRINCE PENDRAGON: "I also hope <i>you</i> find your way and choose to believe in yourself!"
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "...!"
    
    &PORTRAIT_prince pendragon_princependragonbashful
    
    PRINCE PENDRAGON: "I am ready to be sent away now!"
    
    &PORTRAIT_prince pendragon_princependragonsmile
    
    PRINCE PENDRAGON: "Farewell, Plane Walker friends! HOO HOO!!!"
    
    &SHLORP_APPLE_PrincePendragon_out
    
    &PAUSE_0.5
    
    &PAUSE_1.0
    
    &AUDIO_FADEOUT
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "..."
    
    &PORTRAIT_nour_noursmile
    
    * "I'm glad the Prince is feeling better."
    
        &PORTRAIT_nick_nicknervous
    
        NICK: "You helped him see his worth. That was cool..."
        
        &PORTRAIT_nour_noursmile
        
        NOUR: "<i>You</i> did that, not me."
    
    * "I get why you like that game.["] It's kinda cool."
    
        &PORTRAIT_nick_nicksmallsmile
    
        NICK: "...Weirdly enough, it taught me a lot about life."
    
    - &PORTRAIT_ms. suwan_mssuwansmile
    
    MS. SUWAN: "That was a very mature conversation, Nick. You should be proud."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "And also, consider taking the advice you give."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "You've got a lot in common with Prince Pendragon."
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "True. Practice what you preach, Nick."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "..."
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "Thanks."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>Now that Nick is excited about the secret ending...</i>
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: <i>Maybe he'll be willing to talk over a game of Aple Quest.</i>
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "Wanna play Aple Quest?"
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "..."
    
    &PORTRAIT_nick_nickneutral
    
    NICK: "Maybe..."
    
    NOUR: <i>Looks like he's finally ready to talk!</i>
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>All that's left is to go back to TopicSpot and play the game now...</i>
    
    &MUSIC_HallwayMusic
    
    &FOLLOW_NOUR_stop
    
    -> END

=== wertoys_before_aplequest_repeat ===

    With the lack of toys inside, it can hardly be called a toy store anymore.
    
    -> END