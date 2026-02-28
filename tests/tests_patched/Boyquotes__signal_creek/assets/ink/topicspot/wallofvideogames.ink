VAR consoles_start_aplequest = false
VAR nourplaysaplequest = false
=== wallofvg ===
{
    - wallofvg_afterNickRouteComplete:
        -> explored_everything

    - nourplaysaplequest:
        -> wallofvg_afterNickRouteComplete
        
    - consoles_start_aplequest:
        -> wallofvg_nickMad
        
    - else:
        -> wallofvg_beforeNickRoute
}

=== wallofvg_beforeNickRoute ===
//Before shit goes down

        &PORTRAIT_nour_noursmile

    NOUR: "So how many of these have you played?"
    
        &PORTRAIT_nick_nickneutral
    
    NICK: "...Not many, actually."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "This is all big triple-A stuff. They were always too..."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "<i>Too...?</i>"
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "Well... I mean..."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "It doesn't matter if had won extra fancy awards or Game of the Year."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "My mom would <i>kill</i> me if I spent sixty <i>whole</i> dollars on ONE game."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "Right, right..."
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "Looking at these things are the most we can do, and that's not gonna get us out of here."
            
    &PORTRAIT_ms. suwan_mssuwanpondering
            
    MS. SUWAN: "These are display-only, after all."
            
    &PORTRAIT_nick_nicksad
        
    NICK: "Oh... yeah. I guess there's no point in looking at games we can't even play..."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "..."
    
    ->END

=== wallofvg_nickMad ==
//He is refusing to talk after visitng bookshelf.

    &PORTRAIT_nick_nicksad
    
    NICK: "..."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "<i>Residential Villain</i> huh? That's a famous one, right."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "Yes. Even I have dabbled a little in playing. Once or twice."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "They were always my friends' games, however."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "It seems like the price of these games only seem to get more expensive."
    
    &PORTRAIT_nick_nickshadow
    
    NICK: "..."
    
        &PORTRAIT_nour_nourpondering
    
    NOUR: <i>Right, looking at the prices on these things... sixty bucks a game? </i>
    
        &PORTRAIT_nour_nournervous
    
    NOUR: <i>This might be one reason why Nick doesn't let himself enjoy games...</i>
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>It's hard to, knowing the cost.</i>
    
    -> END

=== wallofvg_afterNickRouteComplete ===
//After he visited Console and started talking again.

    &PORTRAIT_nour_nourrealization

    NOUR: "These <i>should</i> be the new releases..."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "But these are all outdated by over a decade."
    
        &PORTRAIT_nick_nickshocked
        
    NICK: "Yeah! They've got the old God of Warcraft."
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "My friend let me play it a little on his SP2."
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "The graphics and story are so top-notch. Definitely deserved all those awards!"
    
        &PORTRAIT_nour_noursmile
        
    NOUR: "<i>I've</i> even heard of how good it is."
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "It makes grown men cry!"
    
        * "What game is this over here?"
            
                &PORTRAIT_nick_nickbigsmile
        
            NICK: "<i>Call of Destiny</i>! A classic."
            
            &PORTRAIT_ms. suwan_mssuwanpondering
            
            MS. SUWAN: "I still recall when this one was released..."
            
            &PORTRAIT_ms. suwan_mssuwanneutral
            
            MS. SUWAN: "That stirred quite a controversy."
            
                &PORTRAIT_nick_nickshocked
            
            NICK: "Wow... You're so old..."
            
                &PORTRAIT_nour_nourannoyed
            
            NOUR: "<i>Nick!</i>"
            
                &PORTRAIT_nick_nickshocked
            
            NICK: "ACK! Sorry..."
            
                &PORTRAIT_mssuwan_mssuwanannoyed
            
            MS. SUWAN: "I prefer the term 'experienced'."
            
                &PORTRAIT_nour_nourpondering
            
            NOUR: "Why do we need like eleven of those, anyway?"
            
                &PORTRAIT_nick_nicksmallsmile
            
            NICK: "Well, it's just cool! You get to shoot big guns and drive tanks and stuff!"
            
            &PORTRAIT_nick_nickbigsmile
            
            NICK: "And they keep making the guns and tanks feel better and better to shoot!"
            
                &PORTRAIT_nour_nourannoyed
            
            NOUR: "...And you're sure it's not military propoganda?"
            
            &PORTRAIT_nick_nicknervous
            
            NICK: "Wait, what?"
            
            &PORTRAIT_nour_nournervous
            
            NOUR: "Uh, nevermind. We can talk about this later..."
            
            &PORTRAIT_nour_noursmile
            
        * "Probably shouldn't spend much more time here."
        
            &PORTRAIT_nour_noursmile
    
    - NOUR: "Ready to go?"
        
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "Yeah."

        -> END

=== explored_everything ===
-> END
