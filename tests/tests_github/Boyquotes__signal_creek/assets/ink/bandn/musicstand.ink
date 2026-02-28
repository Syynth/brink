== musicstand ==

    It's a music stand. Different vintage records are stacked on top of it. 

    {
        - musicstand4:
            -> explored_everything
            
        - chadbrodykristy3:
            -> musicstand4
            
        - musicstand2:
            -> musicstand2_repeat
            
        - chadbrodykristy1 && musicstand1 && cafetables_getsuwantotalk:
            -> musicstand2
            
        - chadbrodykristy1 && musicstand1:
            -> musicstand1_repeat
            
        - chadbrodykristy1:
            -> musicstand1
    }
    
    -> END


== musicstand1 ==
//better onboarding from CBK
    
    One record in particular catches Nour's attention. 
    
    &FOLLOW_NOUR_NourLookAtRecord
    
    &FOLLOW_SUWAN_MsSuwanGoToNour
    
    &FOLLOW_NICK_NickLookAround
    
    &PAUSE_1.0
    
    &EMOTE_NOUR_UpIdle
    
    &EMOTE_NICK_UpIdle
    
    &EMOTE_SUWAN_UpIdle
    
    &PORTRAIT_nour_nourrealization
    
    &EMOTE_NOUR_RightIdle
    
    NOUR: "Hey, Ms. Suwan. You mentioned going to rock shows in the past, right?"
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    &EMOTE_SUWAN_LeftIdle
    
    MS. SUWAN: "...Yes, why?"
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Have you heard of <i>this</i> album, then?"
    
    Nour raises the record of 'Actually, I Do Mind' up to show Ms. Suwan. 
    
    &EMOTE_NOUR_HandRecord
    
    &PAUSE_0.1
    
    &EMOTE_NOUR_RightIdle
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "I used to listen to <i>Paradise</i> all the time when I was younger."
    
    &PORTRAIT_ms. suwan_mssuwansmile
    
    MS. SUWAN: "...Yes. I remember that one. Good album."
    
    &PORTRAIT_nick_nickshocked
    
    &EMOTE_NOUR_RightIdle
    
    &FOLLOW_NICK_NickLook
    
    &PAUSE_1.0
    
    &EMOTE_NICK_UpIdle
    
    NICK: "You listen to <i>rock</i>?" // (SHOCKED)
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    &EMOTE_SUWAN_DownIdle
    
    MS. SUWAN: "Yes."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "Well... used to."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "Oh... that's, wow!"
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    &EMOTE_SUWAN_LeftIdle
    
    MS. SUWAN: "Why are you looking at me like that?" // (FROWNING)
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "Uh, well..."
    
    &PORTRAIT_nour_nourneutral
        
        * "I didn't expect you to listen to a rock band."
        
            &PORTRAIT_nour_nourpondering
            
            NOUR: "You don't really seem like the type - no offense." 
            
            &PORTRAIT_ms. suwan_mssuwansad
            
            MS. SUWAN: "I was very different back then."
            
            &PORTRAIT_nour_noursmile
            
            NOUR: "Do you listen to them now?"
            
            &PORTRAIT_ms. suwan_mssuwanannoyed
            
            MS. SUWAN: "No. It was a distraction, so I stopped."
            
            &PORTRAIT_nour_noursad
            
            NOUR: <i>She looks upset. Something about this feels really personal...</i>
        
        * "It's nothing. I wasn't going to say anything—"
            
            &PORTRAIT_ms. suwan_mssuwanneutral
        
            &PORTRAIT_ms. suwan_mssuwanannoyed
        
            MS. SUWAN: "You were going to say that I don't seem like the type, right?"
            
            &PORTRAIT_nour_nournervous
    
            NOUR: "Um... yeah. Sorry." // (EMBARASSED)
            
            &PORTRAIT_ms. suwan_mssuwanpondering
            
            MS. SUWAN: "Well, I don't anymore, so you aren't entirely incorrect."
            
            &PORTRAIT_ms. suwan_mssuwanangry
            
            MS. SUWAN: "It was a distraction, so I stopped."
            
            &PORTRAIT_nour_noursad
            
            NOUR: <i>She looks upset. Something about this feels really personal...</i>
    
-   NOUR: "You must've been a student when <i>Paradise</i> released this album, right?"

    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "Right. During school, me and my friends used to..."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "..." // (FROWNING)
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>I have to keep the conversation going somehow...</i>
    
    &PORTRAIT_nour_nourneutral
    
        * "Are you okay?"
        
            &PORTRAIT_ms. suwan_mssuwansad
            
            MS. SUWAN: "I'm fine. Paradise just reminds me of hard times. That's all."
            
            &PORTRAIT_ms. suwan_mssuwansad
            
            &PORTRAIT_nick_nicksmallsmile
            
            NICK: "Kind of ironic, given the name." // (HAPPY)
            
            &EMOTE_SUWAN_DownIdle
            
            &PORTRAIT_ms. suwan_mssuwanannoyed
            
            MS. SUWAN: "..." // (ANGRY)
            
            &PORTRAIT_nick_nicknervous
            
            NICK: "...Sorry." // (POUTING)

        * "You went to Seton Hall University, right?"
        
            &PORTRAIT_ms. suwan_mssuwanneutral
    
            MS. SUWAN: "You remember correctly."
            
            &PORTRAIT_ms. suwan_mssuwanpondering
            
            MS. SUWAN: "You asked me this during our interview, remember?"
            
            &PORTRAIT_nour_nourpondering
            
            NOUR: "Yeah... we didn't go into it that much, though."
            
            &PORTRAIT_nour_noursmile
            
            NOUR: "Did you like it?"
            
            &PORTRAIT_ms. suwan_mssuwansad
            
            MS. SUWAN: "...It was challenging, to say the least."
            
            &PORTRAIT_ms. suwan_mssuwanneutral
            
            MS. SUWAN: "But my education there got me where I needed to be."
            
        * "What was it that you and your friends used to do?"
        
            &PORTRAIT_ms. suwan_mssuwanshadow
            
            MS. SUWAN: "..."
    
    - &PORTRAIT_nour_noursad
    
    NOUR: "Did you listen to this with your friends?"
    
    &EMOTE_SUWAN_LeftIdle
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "...Yes. Music was one of the few things that made me happy then."
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "It was one of the only things I could understand at the time."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "After moving to New Jersey from Thailand, I came to law school..."
    
    &PORTRAIT_ms. suwan_mssuwanshadow
    
    MS. SUWAN: "My English was still not perfect."
     
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "But I went to school regardless."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "Since the four of us lived on the same floor, we became close." 
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "We used to play Paradise while we studied together."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "I think I owe Paradise my first real friendships." // (SAD)
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "Rocking out led to your friendships?"
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "That's so cool..." // (EXCITED)
    
    &EMOTE_SUWAN_DownIdle
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "But that was a different time. Things change."
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "How 'bout we rock out right now then? For old time's sake?" // (EXCITED)
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "I don't think that would be a good use of our time."
    
        * "Do you mind if I play this album? I'd like to listen to it."
            
        * "Maybe we could play it for Nick?"
        
            &PORTRAIT_nour_nourpondering
        
            NOUR: "I don't think he's ever even <i>heard</i> of Paradise, let alone any of their songs."
            
            &PORTRAIT_nick_nicknervous
            
            NICK: "Uh..."
            
            &PORTRAIT_nick_nicksmallsmile
            
            NICK: "Guilty."
            
            &PORTRAIT_nick_nickpondering
            
            NICK: "I just know that they do rock... stuff?"
            
            &PORTRAIT_nour_noursmile
            
            NOUR: "Knew it."
            
            &PORTRAIT_nour_nourrealization
            
            NOUR: "Besides, hearing something on a record is always way better."
            
            &PORTRAIT_ms. suwan_mssuwanpondering
            
            MS. SUWAN: "That is true."
            
            &PORTRAIT_nick_nickshocked
            
            NICK: "Wait, things sound different on records?"
            
            &PORTRAIT_nick_nickbigsmile
            
            NICK: "Can I hear it? Please?" // (EXCITED)

    - &PORTRAIT_ms. suwan_mssuwannervous
    
    &EMOTE_SUWAN_UpIdle
    
    MS. SUWAN: "..."
    
    Ms. Suwan sighs and steps back.
    
    &FOLLOW_SUWAN_SuwanStepBack// MS SUWAN STEPS BACK SLIGHTLY FROM THE MUSIC STAND
    
    &PAUSE_0.4
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "Fine. Go ahead."
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "Time to put on a classic... <i>Sounds like Old Angst</i>!" 
    
    &FOLLOW_NOUR_NourPutInAlbum
    
    &PAUSE_1.0
    
    &EMOTE_NOUR_Gaming
        
    &PAUSE_1.0
    
    &FOLLOW_NOUR_NourLookAtRecord
    
    &PAUSE_1.0
    
    &AUDIO_FADEOUT
    
    The record enters the record player. Rock music fills the store...
    
    &EMOTE_NOUR_LookAround
    
    &PAUSE_1.0

    &MUSIC_MusicStandMusic
    
    &EMOTE_NICK_LookAround
    
    &EMOTE_SUWAN_LeftIdle
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "This song..."
    
    &PORTRAIT_ms. suwan_mssuwansmile
    
    MS. SUWAN: "..."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: <i>She seems... relaxed, listening to the music.</i>
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>Maybe I can ask her about her friends now?</i>
    
    &PORTRAIT_nour_nournervous
    
    NOUR: <i>But... carefully.</i>
    
    &PORTRAIT_nour_noursmile
    
    &FOLLOW_NOUR_NourGoToSuwan
    
    &PAUSE_1.0
    
    &EMOTE_NOUR_RightIdle
    
    NOUR: "So, your friends... Do you still talk to them?"
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "Not anymore."
    
    &EMOTE_NICK_UpIdle
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "What happened?"
    
    &PORTRAIT_nour_nourannoyed
    
    &EMOTE_NOUR_DownIdle
    
    NOUR: "Nick!" // (SHOCKED)
    
    &PORTRAIT_nick_nickangry
    
    NICK: "What? I wanna know!" // (ANGRY)
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "..."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    &EMOTE_SUWAN_DownIdle
    
    MS. SUWAN: "...You really want to know?"
    
    &PORTRAIT_ms. suwan_mssuwanfurious
    
    MS. SUWAN: "Fine. I'll tell you."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "Oh... uh..."
    
    &PORTRAIT_nour_nournervous
    
    &EMOTE_NOUR_RightIdle
    
    NOUR: "Um, go ahead..."
    
    &PORTRAIT_ms. suwan_mssuwanangry

    MS. SUWAN: "Listening to music doubled the time it took me to complete my assignments."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "I struggled to catch up to my friends because I was still learning English at the time."
    
    &PORTRAIT_ms. suwan_mssuwanfurious

    MS. SUWAN: "Once, my friends got upset at me for not going to a concert with them, right around finals."
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "I chose not to fail my finals over the concert in the end."
    
    &PORTRAIT_ms. suwan_mssuwanfurious
    
    MS. SUWAN: "They didn't care about my situation, so I stopped caring about them."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "I stopped hanging out with them, they started going to concerts without me..."
    
    &PORTRAIT_ms. suwan_mssuwanshadow
    
    MS. SUWAN: "And the rest is history." // (SAD)
    
    &PORTRAIT_nick_nicksad
    
    NICK: "That really sucks." // (SAD)
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "There is nothing I can do about it anymore."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "I haven't talked to them since that fight."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "Have you <i>thought</i> about them since that fight?"
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "..."
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    &PORTRAIT_nour_nournervous
    
    &FOLLOW_SUWAN_MsSuwanMoveBack
    
    &PAUSE_1.0
    
    &EMOTE_SUWAN_RightIdle
    
    MS. SUWAN: "Can you turn that off? It's giving me a splitting headache." // (ANGRY)
    
    NOUR: <i>She's avoiding all of her problems again.</i>
    
    &EMOTE_NOUR_UpIdle
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: <i>Just when I thought we were getting somewhere...</i>
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>If she could make amends with her old friends...</i>
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>Maybe they'll help us get out of here.</i>
    
    &PORTRAIT_ms. suwan_mssuwanannoyed

    MS. SUWAN: "Nour. Turn the music off. Now." // (ANGRY)
    
    &PORTRAIT_nour_nourannoyed
    
        * "Fine, I'll turn it off."
            
            &MUSIC_STOP
            
            &PAUSE_1.0
            
            &PORTRAIT_ms. suwan_mssuwansad

            MS. SUWAN: "Finally."
            
            &PORTRAIT_ms. suwan_mssuwannervous
            
            MS. SUWAN: "Let's stop talking about my past — it isn't helping us get anywhere."
            
            &PORTRAIT_ms. suwan_mssuwanneutral
            
            MS. SUWAN: "Namely, out of here."
            
            &PORTRAIT_nour_nourannoyed
            
            NOUR: "You not talking about it is hurting us MORE."
            
            &PORTRAIT_ms. suwan_mssuwanangry
            
            MS. SUWAN: "Excuse me?"
            
            &PORTRAIT_nour_nourangry
            
            NOUR: "You won't talk to the people who might be able to help us get out of here!" // (ANGRY)
            
            &PORTRAIT_ms. suwan_mssuwanfurious
            
            MS. SUWAN: "They don't know anything. Leave it alone, Nour."
            
            &PORTRAIT_ms. suwan_mssuwanshadow
            
            MS. SUWAN: "<i>We're leaving.</i>"
        
        * "..."
        
            &PORTRAIT_ms. suwan_mssuwanangry
            
            &PORTRAIT_ms. suwan_mssuwanannoyed
            
            MS. SUWAN: "...Nour. Did you hear what I just said?" // (ANGRY)
            
            &PORTRAIT_nour_nourannoyed
            
                ** "Yeah, and I'm choosing to ignore you!" // (ANGRY)
                    
                    &PORTRAIT_ms. suwan_mssuwanrealization
                    
                    MS. SUWAN: "..." // (SHOCKED)
                    
                    &PORTRAIT_ms. suwan_mssuwanfurious
                
                    MS. SUWAN: "Nour, I am the <i>adult</i> here, and part of the <i>law</i>. You <i>will</i> listen to me." // (ANGRY)
                    
                    &PORTRAIT_nour_nourangry
                
                    NOUR: "Except <i>you're</i> the one acting immature right now!" // (ANGRY)
                    
                    &MUSIC_STOP
                    
                    Ms. Suwan shuts the record off.

                
                ** "You want it louder?"
                    
                    &VOLUME_0.0
                    
                    &PORTRAIT_ms. suwan_mssuwanangry
                    
                    MS. SUWAN: "Don't you dare." // (ANGRY)
                    
                    *** [Turn it up louder]
                    
                        &VOLUME_10.0
                    
                        &PORTRAIT_ms. suwan_mssuwanfurious
                        
                        MS. SUWAN: "This is <i>extremely</i> immature of you. I'm very disappointed."
                    
                        &PORTRAIT_nour_nourangry
                        
                        NOUR: "I can't hear you over this <i>great music</i> that's supposed to <i>bring people together</i>." // (ANGRY)
                        
                        &PORTRAIT_ms. suwan_mssuwanshadow
                        
                        MS. SUWAN: "..."
                        
                        &MUSIC_STOP
                        
                        Ms. Suwan shuts the record off.
    
    -  &PORTRAIT_nour_nourannoyed
    
    &FOLLOW_NOUR_NourMoveBack
    
    &PAUSE_1.0
    
    &EMOTE_NOUR_RightIdle
    
    NOUR: "You can't keep avoiding your friends, only to get all sad and vague about it!" // (ANGRY)
    
    &EMOTE_NICK_RightIdle
    
    &PORTRAIT_ms. suwan_mssuwanrealization
    
    MS. SUWAN: "..." // (SHOCKED)
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "You keep talking about how hard you worked for everything in your life."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "You haven't backed down from ANYTHING!" // (ANGRY)
    
    &PORTRAIT_nour_nourangry
    
    NOUR: "How could YOU, of all people, be afraid of a <i>conversation</i>?" // (ANGRY)
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "You cannot understand what this is like."
    
    &PORTRAIT_ms. suwan_mssuwanfurious
    
    MS. SUWAN: "To look back <i>decades</i>, knowing you cannot change a thing..."
    
    &PORTRAIT_ms. suwan_mssuwanshadow
    
    MS. SUWAN: "I <i>refuse</i> to put myself is in such a hopeless, <i>helpless</i> place again."
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "The best thing to do..."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "The <i>only</i> thing I can do..."
    
    &PORTRAIT_ms. suwan_mssuwanshadow
    
    MS. SUWAN: "...is to forget it and move on."
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "Did you though?"
    
    &PORTRAIT_ms. suwan_mssuwanfurious
    
    MS. SUWAN: "Did I <i>what</i>?"
    
    &PORTRAIT_nour_noursad
    
    NOUR: "Did you forget about it? Did you move on?"
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "How dare you!"
    
    &PORTRAIT_ms. suwan_mssuwanrealization
    
    MS. SUWAN: "..." // (SAD)
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "I..."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "Sometimes you need to confront your past."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "Even if you know you can't change it."
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "I promise you, carrying this with you and bottling up your feelings?" 
    
    &PORTRAIT_nour_noursad
    
    NOUR: "It's only gonna hurt you more."
    
    &PORTRAIT_ms. suwan_mssuwanshadow
    
    &PORTRAIT_nour_noursad
    
    NOUR: "Hurt <i>us</i> more." 
    
    MS. SUWAN: "..." // (SAD)
    
    &PORTRAIT_nour_noursmile
    
        * "I'll be right by your side."
        
        * "If none of this is real, there's no risk involved, right?"
        
        * "Do it for yourself."
    
    - &PORTRAIT_nour_nourneutral
    
    NOUR: "I think working out your feelings with them could help you and your past feelings."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "What d'you say?"
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "..."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    &EMOTE_SUWAN_LeftIdle
    
    MS. SUWAN: "...Fine. I'll talk to them."
    
    &PORTRAIT_nour_noursmile
    
    &MUSIC_BandNMusic
    
    NOUR: <i>I finally managed to convince her to confront them.</i>
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>We should go find Chad, Brody, and Kristy again.</i>
    
    &FOLLOW_NOUR_stop

    -> END

//figure out how to implement intervals for when other items are not collected
== musicstand1_repeat ==
    
    NOUR: <i>It'd be nice for Ms. Suwan to get back into her old hobby.</i>
    
    NOUR: <i>But I don't think she's quite ready yet.</i>
    
    NOUR: <i>I can come back to this later.</i>
    
    NOUR: <i>For now, we should go to Chad, Brody, and Kristy...</i>
    
    NOUR: <i>before she changes her mind about talking to them.</i>
    
    -> END

== musicstand2 ==

    &PORTRAIT_ms. suwan_mssuwanangry

    MS. SUWAN: "..."
    
    &PORTRAIT_ms. suwan_mssuwanfurious
    
    MS. SUWAN: "Why are we here again?" // (ANGRY)
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "You know, I used to listen to Paradise with my ex-friend, too."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "Sometimes it hurts to listen to, which is why I hadn't played it in so long." // (SAD)
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "..."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "Do you think it's possible?"
    
    &PORTRAIT_ms. suwan_mssuwanshadow
    
    MS. SUWAN: "To still love the things that remind you of your mistakes?"
    
    &PORTRAIT_nour_nourneutral
        
        * "You have to accept the pain either way."
            
            &PORTRAIT_nour_noursad
        
            NOUR: "Doing that will let you remember all the goods things that came out of it, too."
            
            &PORTRAIT_nour_noursmile
            
            NOUR: "Like the happiness music gave you."
            
        * "Nothing should stop you from enjoying what you love."
        
            &PORTRAIT_nour_noursad
        
            NOUR: "It'd be a shame to not let yourself enjoy Paradise because of what happened."
            
            &PORTRAIT_nour_noursmile
            
            NOUR: "Don't let your past stop you from finding joy in what you've always loved."
            
        * "We can't carry our regrets with us forever."
        
            &PORTRAIT_nour_noursad
        
            NOUR: "Some things work out, and some don't."
            
            &PORTRAIT_nour_nournervous
            
            NOUR: "But it doesn't mean you've failed or that you're ruined forever."
            
            &PORTRAIT_nour_nourneutral
            
            NOUR: "You can free yourself of the guilt by acknowledging that you aren't perfect."
            
            &PORTRAIT_nour_noursmile
            
            NOUR: "It's ok to forgive yourself."
    
    - &PORTRAIT_nour_nourneutral
    
    NOUR: "You can't change what happened..."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "But that doesn't mean you should avoid it entirely."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "Plus, you have the opportunity to do that anyway."
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "Thanks to this weird mall."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "And if none of this is real like you say it is..."
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "Then, the stakes are relatively low, right?"
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "I suppose you're right about that."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "Oh! I have an idea, actually."
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "Why don't you take an album with you?"
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "You're not a stressed out student anymore. You could rock out once in a while."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "... I'd rather not take something without paying for it."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "There's no way after everything we've been through..."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "You <i>still</i> think this place is part of our reality?"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "This place doesn't even have a chashier to buy it from!"
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "... Alright."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "Which one should I take?"
    
    // NOUR STEPS FORWARD TOWARDS THE MUSIC STAND AND TAKES ONE OF THE ALBUMS, THEN TURNS BACK AROUND AND GOES TOWARDS MS SUWAN, STOPPING IN FRONT OF HER AND GIVING HER THE ALBUM.
    
        * [<i>Actually, I Do Mind</i>]
            Nour hands Ms. Suwan <i>Actually, I Do Mind</i>. 
            
        * [<i>Stain</i>]
            Nour hands Ms. Suwan <i>Stain</i>. 
            
        * [<i>Out Birth</i>]
            Nour hands Ms. Suwan <i>Out Birth</i>. 
            
// MS SUWAN REACHES OUT AND TAKES THE ALBUM.
    
    - &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "Thank you, Nour."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "I'll take this album... and go apologize to them all."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "I don't know if they'll accept my apology."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "But I have to try." // (SAD)
    
    &PORTRAIT_nour_noursmile
    
    NOUR: <i>It looks like Ms. Suwan is ready to apologize to Chad, Brody and Kristy.</i>
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>We should go to them now.</i>
        
    &LIGHT_Suwan1
        
    -> light_on

== musicstand2_repeat ==

    NOUR: <i>Ms. Suwan's ready to apologize to Chad, Brody and Kristy. We should go to them now.</i>
    
    -> END

== musicstand4 ==

    &PORTRAIT_nour_noursad

    NOUR: "I wish I could've gone to a Paradise concert. It must've been so cool to hear them live." // (SAD)
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "It was. Nothing like the digital recordings. 
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "The energy isn't the same."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "I should've gone to more concerts when I had the chance." // (SAD)
        * "It's not too late."
        
            NOUR: "If you discover newer rock bands you like, you could go to their concerts."
            
            NOUR: "And older bands do reunion concerts, too." 
            
            &PORTRAIT_nour_noursmile
            
            NOUR: "Whatever the future holds, I know there's a rock concert waiting for you."
            
        * "At least we still have these records."
        
            &PORTRAIT_nour_noursmile
        
            NOUR: Nothing like having some vinyls to really get into the music."
            
            NOUR: "I bet you could even find live recordings, too."

    - MS. SUWAN: "I'll keep that in mind."
    
    &PORTRAIT_ms. suwan_mssuwansmile
    
    MS. SUWAN: "Thank you, Nour."
    
        -> END
