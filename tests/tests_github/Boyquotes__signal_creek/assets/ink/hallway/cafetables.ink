== cafetables ==
//note to self: do some gathers - choices that lead to the same conversation 

    It's a table with coffee cups left on it. There's still steam coming out of the holes in the lid.
    
    &FOLLOW_NICK_NickCafeStart
    
    &FOLLOW_SUWAN_SuwanCafeStart
    
    &FOLLOW_NOUR_NourCafeStart
    
    &PAUSE_0.7
    
    &EMOTE_NICK_DownIdle
    
    &EMOTE_NOUR_LeftIdle
    
    &EMOTE_SUWAN_UpIdle

{
    - cafetables3:
        -> explored_everything
        
    - chadbrodykristy2 && musicstand2 && chadbrodykristy3: 
        -> cafetables3

    - chadbrodykristy2 && musicstand2:
        -> cafetables3_5
        
    - cafetables_getsuwantotalk:
        -> cafetables2_5_repeat

    - chadbrodykristy2 && beautystand_find_pinklady && cafetables1:
        -> cafetables2_5
        
    - chadbrodykristy2 && beautystand_find_pinklady:
        -> cafetables2
        
    - else: 
        -> cafetables1
}

== cafetables1 ==

    &PORTRAIT_nour_noursad

    NOUR: "Didn't think I'd see this cafe here..."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "It's even in good condition."
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "I still refuse to think any of this is real."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "I was <i>right here</i> before confirming the demolition a few weeks ago."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "Yet everything is still here. Though in much better condition."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "...Like how I remember they used to be."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "You've been to this cafe before?"
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "You don't strike me as the type of person to sit and chill in a mall."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "A long time ago. It was the only table big enough for... a group."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "Yeah, I remember this being a good spot in the cafe." 
    
    &PORTRAIT_nour_noursad
    
    NOUR: "..."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "I, uh. Used to come here with someone, too. A friend."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "..."
    
    MS. SUWAN: "We should move on."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "No use in dwelling over... Never mind."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "I... Okay."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "What happened at this cafe for the both of you?"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "You know what? Nevermind..."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "I'll, uh... I'll stop there."
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>Thinking about all the old memories I have here...</i>
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>It hurts more than I thought it would.</i>
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: <i>Seems like we both have things we want to avoid.</i>
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>Is that the right thing to do, though... ?</i>
    
    -> END

== cafetables2 ==

    &PORTRAIT_nour_nourrealization

    NOUR: "Didn't think I'd see this cafe here. It's even in good condition."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Seems like anything is possible in this place..."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "Same chairs and tables... but they look newer."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "Just like how I remember them."
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "But why are we here, Nour?"
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "Oh. Um, well... Did you come here with them?"
    
    &PORTRAIT_nour_noursad
    
    NOUR: "Those three from before?"
    
    //&PORTRAIT_ms. suwan_mssuwanshadow
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "I don't want to talk about this or them anymore. It's a complete waste of time."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "Ms. Suwan... Are you really okay?"
    
    &PORTRAIT_ms. suwan_mssuwanshadow

    MS. SUWAN: "I'm fine. Just stop asking questions. It's over now."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "You don't look so good though..."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "Ms. Suwan...?"
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "I..."
    
    &PORTRAIT_ms. suwan_mssuwanshadow
    
    MS. SUWAN: "..."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>For her to not even be able to hide that look on her face...</i>
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>That means those three really did mean a lot to her.</i>
    
    &PORTRAIT_nour_nournervous
    
    NOUR: <i>I definitely know how that feels... a friendship gone wrong.</i>
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>She can't keep avoiding this conversation.</i>
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>Even if she thinks it's the easier choice...</i>
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>Even Nick noticed that she isn't okay.</i>
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>But how do I get her to talk about what happened?</i>
    
    -> cafetables_getsuwantotalk

== cafetables2_5 ==

    &PORTRAIT_nour_nournervous

    NOUR: "Did you come here with them? Those three from before?" 
    
    &PORTRAIT_nour_noursad
    
    NOUR: "They seemed like they, well... cared about you."
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "I don't want to talk about this or them anymore. Stop prying."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "Ms. Suwan... Are you really ok?"
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "I'm fine. Just stop asking questions."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "It's over now."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "You don't look so good though..."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "Ms. Suwan..?"
    
    &PORTRAIT_ms. suwan_mssuwanshadow

    MS. SUWAN: "..."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>For her to not even be able to hide that look on her face...</i>
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>That means those three really did mean a lot to her.</i>
    
    &PORTRAIT_nour_nournervous
    
    NOUR: <i>I definitely know how that feels... A friendship gone wrong.</i>
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>She can't keep avoiding this conversation.</i>
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>Even if she thinks it's the easier choice...</i>
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>Even Nick noticed that she isn't okay.</i>
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>But how do I get her to talk about what happened?</i>
    
    ->cafetables_getsuwantotalk

== cafetables_getsuwantotalk ==

    &PORTRAIT_nour_nournervous

    * "We need to talk about what happened."
        
        &PORTRAIT_nour_nourpondering
        
        NOUR: "They used to be your friends, right?"
        
        &PORTRAIT_ms. suwan_mssuwanangry
        
        MS. SUWAN: "Are you serious? I said <i>leave</i> it."
        
        &PORTRAIT_nour_nourannoyed
        
        NOUR: "You clearly did <i>something</i> — you're just not telling us the whole story."
        
        &PORTRAIT_nour_noursad
        
        NOUR: "Both sides matter, and—"
        
        &PORTRAIT_ms. suwan_mssuwanangry
        
        MS. SUWAN: "There is nothing to discuss."
        
        &PORTRAIT_nour_nourannoyed
        
        NOUR: "You were rude to them! We heard everything." 
        
        &PORTRAIT_nour_nourangry
        
        NOUR: "Having a conversation about what happened could actually help us get out of here!"
        
        &PORTRAIT_ms. suwan_mssuwanannoyed
        
        MS. SUWAN: "Conversation? This is a one-sided interrogation."
        
        &PORTRAIT_ms. suwan_mssuwanangry
        
        MS. SUWAN: "You have some nerve, Nour."
        
        &PORTRAIT_ms. suwan_mssuwanfurious
        
        MS. SUWAN: "I've repeatedly told you to drop the subject."
        
        &PORTRAIT_ms. suwan_mssuwanannoyed
        
        MS. SUWAN: "Clearly, your meddling know no bounds."
        
        &PORTRAIT_ms. suwan_mssuwanangry
        
        MS. SUWAN: "The way I deal with things is <i>my</i> business."
        
        &PORTRAIT_ms. suwan_mssuwanfurious
        
        MS. SUWAN: "I don't need the opinions of someone who just watched me go through a humiliating ordeal."
        
        &PORTRAIT_ms. suwan_mssuwanshadow
        
        MS. SUWAN: "You would hardly understand what I went through, anyway."
        
        &PORTRAIT_nour_nourannoyed
        
        NOUR: "..."
        
        &PORTRAIT_nour_noursad
        
        NOUR: <i>Ugh, that went terribly. Being direct just made her even more hostile.</i>
        
        &PORTRAIT_nour_nourannoyed
        
        NOUR: <i>And I DO understand her! Way more than she thinks.</i>
        
        &PORTRAIT_nour_nournervous
        
        NOUR: <i>Maybe I should try approaching this differently...</i>
            
            -> cafetables_getsuwantotalk
    
    * "I... think I can relate to what may've happened."
    
        &PORTRAIT_ms. suwan_mssuwannervous
        
        MS. SUWAN: "..."
        
        &PORTRAIT_nour_noursad
        
        NOUR: "I used to come here with an old friend."
        
        &PORTRAIT_nour_nourneutral
        
        NOUR: "This was our favorite spot in the cafe."
        
        &PORTRAIT_nour_nourpondering
        
        NOUR: "Perfect amount of sun, non-rickety tables..."
        
        &PORTRAIT_nour_noursmile
        
        NOUR: "We never wanted to go home and do homework."
        
        &PORTRAIT_nour_nourneutral
        
        NOUR: "We would imagine staking it out here in the cafe."
        
        &PORTRAIT_nour_noursmile
        
        NOUR: "And we'd ration pastries and live off of our favorite lattes."
        
        &PORTRAIT_nour_nourneutral
        
        NOUR: "..."
        
        &PORTRAIT_nour_noursad
        
        NOUR: "...We used to imagine that, anyway."
        
        &PORTRAIT_nick_nicknervous
        
        NICK: "So how did it go? Did you ever get to do it?"
        
        &PORTRAIT_nour_noursad
        
        NOUR: "No... We had a falling out. It was bad. I don't talk to her anymore."
        
        &PORTRAIT_ms. suwan_mssuwansad
        
        MS. SUWAN: "..."
        
        &PORTRAIT_ms. suwan_mssuwannervous
        
        MS. SUWAN: "I didn't know that."
        
        &PORTRAIT_nour_nourneutral
        
        NOUR: "It's easier to avoid what happened, but ignoring the good memories hurts, too."
        
        &PORTRAIT_nour_noursad
        
        NOUR: "Like this one here."
        
        &PORTRAIT_nour_nourannoyed
        
        NOUR: "Losing that friendship messed me up for a long time."
        
        &PORTRAIT_nour_nourneutral
        
        NOUR: "I got my homework done quicker, sure. But honestly speaking?"
        
        &PORTRAIT_nour_noursad
        
        NOUR: "She made it a lot easier."
        
        &PORTRAIT_ms. suwan_mssuwansad        
        
        MS. SUWAN: "...They were like that for me, too. Made school easier."
        
        &PORTRAIT_nour_noursmile
        
        NOUR: "I guess that's the point of having friends, right?"
        
        &PORTRAIT_ms. suwan_mssuwanangry 
        
        MS. SUWAN: "But they were distractions for me in the end. Our hobbies, our hangouts..."
        
        &PORTRAIT_ms. suwan_mssuwanneutral
        
        MS. SUWAN: "It was all time and energy I knew I could use for my career."
        
        &PORTRAIT_ms. suwan_mssuwannervous
        
        MS. SUWAN: "So, I made my decision. When they were gone, I had <i>everything</i>..."
        
        &PORTRAIT_ms. suwan_mssuwanpondering
        
        MS. SUWAN: "175 on my LSAT, a stacked resume, and a promising career."
        
        &PORTRAIT_nour_nournervous
        
        NOUR: "Was it worth it?"
        
        &PORTRAIT_ms. suwan_mssuwannervous
        
        MS. SUWAN: "Objectively speaking, yes. I got everything I wanted."
        
        &PORTRAIT_nick_nickpondering
        
        NICK: "Huh... And that made you happy?" 
        
        &PORTRAIT_nick_nicksad
        
        NICK: "You don't... like... y'know, regret anything?"
        
        &PORTRAIT_ms. suwan_mssuwanangry
        
        MS. SUWAN: "Don't be silly. I have a fulfilling life."
        
        &PORTRAIT_nick_nickshocked
        
        NICK: "Then what do you do for fun? What do you do that makes you happy?"
        
        &PORTRAIT_ms. suwan_mssuwanneutral
        
        MS. SUWAN: "That's easy. I—"
        
        &PORTRAIT_nick_nickangry
        
        NICK: "NOT related to your lawyer stuff. FUN stuff."
        
        &PORTRAIT_nick_nicknervous
        
        NICK: "Like, what do you do after you come home from lawyer-ing?"
        
        &PORTRAIT_ms. suwan_mssuwanpondering
        
        MS. SUWAN: "..."
        
        &PORTRAIT_ms. suwan_mssuwannervous
        
        MS. SUWAN: "I... I don't. I just go to sleep to wake up early the next day."
        
        &PORTRAIT_ms. suwan_mssuwansad
        
        MS. SUWAN: "On repeat."
        
        &PORTRAIT_nour_nourpondering
        
        NOUR: "Not even a hobby?"
        
        &PORTRAIT_ms. suwan_mssuwanpondering
        
        MS. SUWAN: "Well... I used to. But not anymore." 
        
        &PORTRAIT_ms. suwan_mssuwannervous
        
        MS. SUWAN: "Not for a long time..."
        
        &PORTRAIT_ms. suwan_mssuwansad
        
        MS. SUWAN: "..."
        
        &PORTRAIT_ms. suwan_mssuwanshadow
        
        MS. SUWAN: "...God."
        
        &PORTRAIT_nick_nicksad
        
        NICK: "..."
        
        &PORTRAIT_nour_noursad
        
        NOUR: <i>Seems like she was shaken up by Nick asking what she did for fun....</i>
        
        &PORTRAIT_nour_nournervous
        
        NOUR: <i>Her past pain shouldn't hold her back from the things that used to make her happy.</i>
        
        &PORTRAIT_nour_noursad
        
        NOUR: <i>They still could.</i>
        
        &PORTRAIT_nour_nourpondering
        
        NOUR: <i>She might not be ready to face her ex-friends...</i>
        
        &PORTRAIT_nour_nourrealization
        
        NOUR: <i>But maybe I could open her up to the idea.</i>
        
        &PORTRAIT_nour_nourneutral
        
        NOUR: <i>I should show her something that she used to love with her friends...</i>
        
        &PORTRAIT_nour_noursmile
        
        NOUR: <i>...Because I think she's ready to hear it again.</i>
            -> END
    
    * "So... Do you like going to cafes?"
    
        &PORTRAIT_ms. suwan_mssuwanpondering
        
        MS. SUWAN: "No. It's more efficient and practical to make my own coffee."
        
        &PORTRAIT_nour_noursmile
        
        NOUR: "Do you ever drink coffee with, like... other people?"
        
        &PORTRAIT_ms. suwan_mssuwannervous
        
        MS. SUWAN: "No."
        
        &PORTRAIT_nour_nourpondering
        
        NOUR: "Have you always preferred it that way? Spending time alone?"
        
        &PORTRAIT_ms. suwan_mssuwanannoyed
        
        MS. SUWAN: "...Yes."
        
        &PORTRAIT_nour_nourrealization
        
        NOUR: "Did you ever—"
        
        &PORTRAIT_ms. suwan_mssuwanangry
        
        MS. SUWAN: "Enough of these pointless questions. We need to focus on more pressing matters."
        
        &PORTRAIT_nour_nournervous
        
        NOUR: <i>That didn't go too well. Being indirect got me nowhere near the subject.</i>
        
        &PORTRAIT_nour_nourpondering
        
        NOUR: <i>Maybe I should try approaching this differently...</i>
            
        -> cafetables_getsuwantotalk
    
    // + [I have nothing to say] I'll try to get her to talk later.
        
    //     -> END
== cafetables2_5_repeat ==

    &PORTRAIT_nour_nourneutral

    NOUR: <i>I got through to her with an old memory of her friends.</i>
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>Maybe she's ready to revisit another old love of hers with a new perspective.</i>
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: <i>I wonder... Is she ready to listen to Paradise again?</i>
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>We should head back to B&N to find out.</i>
    
    -> END

== cafetables3_5 ==
    // MS. SUWAN: "Let's not stop here. I'm ready to talk to the three of them."
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>I should go find Chad, Kristy, and Brody.</i>
       
        -> END

== cafetables3 ==

    &PORTRAIT_ms. suwan_mssuwanpondering

    MS. SUWAN: "Now thinking back, I had some good memories at this table too."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "Really?"
    
    &PORTRAIT_ms. suwan_mssuwansmile
    
    MS. SUWAN: "Chad, Kristy, Brody, and I would discuss the concepts behind Paradise's albums."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "We would pool in money to afford limited edition shirts and CDs..."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "And we were notorious for being rowdy and hounding the cafe owner into playing Paradise."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "Yo, you were a <i>rebel!</i>"
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "I wish I could've seen that!"
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "Did it work?"
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "I don't know. We were banned from the cafe for being 'disruptive to the public'."
    
    &PORTRAIT_ms. suwan_mssuwansmile
    
    MS. SUWAN: "I suppose bringing our own CD players to play Paradise in protest didn't help."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "I can't believe it..."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "What?"
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "You were a PUNK back then!"
    
    NICK: "That's so COOL!"
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "Don't get any wrong ideas. You shouldn't behave that way in public."
    
    &PORTRAIT_ms. suwan_mssuwansmile
    
    MS. SUWAN: "...But of course, establishments <i>should</i> listen to the people's demands."
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "Agreed. Plus, I don't think your actual job is totally different from that ideal, either."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "A lawyer states their case and persuades others for their cause."
    
    &PORTRAIT_ms. suwan_mssuwansmile
    
    MS. SUWAN: "I just... spend my time doing it at the office rather than a mall's cafe."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>It sounds like this cafe really did hold precious memories for her.</i>
    
    &PORTRAIT_nour_nournervous
    
    NOUR: <i>It did for me, too...</i>
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>Would I have been happier if I had ended things differently with my friend?</i>
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "Nour? Are you ok?"
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "Huh?"
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "Oh. Yeah. I'm... I'm okay."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "I was just thinking about... something. It's nothing."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "That didn't seem like nothing to me, but if that's what you want to say..."
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>...Somehow, I always end up thinking about her.</i>
    
    -> explored_everything