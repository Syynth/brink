VAR disposition = 0
VAR threat_level = 0  
VAR phase = 1
VAR observed_clues = 0
VAR effective_phase1 = "BRIBE"
VAR effective_phase2 = "CHARM" 
VAR effective_phase3 = "THREATEN"

The rain never stops in Neo-Kowloon. You adjust your damp trench coat as you enter THE JADE LOTUS, a bar where the air tastes like burnt silicon and regret. Behind the counter, a cybernetic gangster cleans a plasma pistol with a rag soaked in synth-whiskey. His neural port glitches—a static halo around his head.
+ [Approach the bar] -> environmental_study // Changed from direct phase_1_clues
+ [Observe the environment (30 seconds)] -> environmental_study

=== environmental_study ===
{
    - observed_clues < 3:
        The gangster ignores your presence, focused on weapon maintenance. You use the time to study the bar:
        + [Inspect gangster's hands] -> servo_whine_clue
        + [Study the credit reader] -> transaction_logs_clue  
        + [Scan neural port] -> firewall_clue
    - else:
        Time's up. The gangster notices your hesitation.
        ~ threat_level += 1
        -> phase_1_clues
}

=== servo_whine_clue ===
His left wrist servo emits a high-pitched whine—overclocked hardware.
~ observed_clues +=1
-> environmental_study

=== transaction_logs_clue ===  
The credit reader flickers with red transaction alerts: [VIOLENCE TAX] [VIOLENCE TAX] [VIOLENCE TAX].
~ observed_clues +=1
-> environmental_study

=== firewall_clue ===
His neural port’s firewall has a 0.3-second lag after each glitch—exploitable.
~ observed_clues +=1
-> environmental_study

=== phase_1_clues ===
The gangster looks up. "You’ve stared long enough."
+ [BRIBE] -> phase_1_bribe
+ [CHARM] -> phase_1_charm
+ [THREATEN] -> phase_1_threaten
+ [HACK] -> phase_1_hack

=== phase_1_bribe ===
{
    - effective_phase1 == "BRIBE":
        ~ disposition +=3
        "Smart." He pockets the cred-stick. "Now talk."
    - else:
        ~ disposition -=2
        ~ threat_level +=1
        "Bribes die here." He crushes the cred-stick.
}
-> phase_1_resolve

=== phase_1_charm ===
{
    - effective_phase1 == "CHARM":
        ~ disposition +=3
        "Cute." His optic flickers blue. "But cute don’t pay rent."
    - else:
        ~ disposition -=1
        "Flattery’s for joygirls." His servo fist clenches.
}
-> phase_1_resolve

=== phase_1_threaten ===
{
    - effective_phase1 == "THREATEN":
        ~ disposition +=3
        "Finally—someone with spine." The pistol stays holstered.
    - else:
        ~ disposition -=3
        ~ threat_level +=2
        "Wrong gamble." The pistol’s muzzle glows.
}
-> phase_1_resolve

=== phase_1_hack ===
{
    - effective_phase1 == "HACK":
        ~ disposition +=3
        "Sloppy... but effective." His head jerks as you breach the port.
    - else:
        ~ disposition -=4
        ~ threat_level +=3
        "Firewalls exist for pests." Your glove smokes.
}
-> phase_1_resolve

=== phase_1_resolve ===
{
    - threat_level >= 3:
        -> death
    - disposition >= 3:
        ~ phase = 2
    ~ observed_clues = 0
    -> phase_2_transition
    - else:
        ~ observed_clues = 0
        -> phase_1_clues
}

=== phase_2_transition ===
The gangster pours two shots. "You survived phase one. Now—why should I care?"
+ [Negotiate carefully] -> phase_2_clues
+ [Push luck] -> phase_2_risky

=== phase_2_risky ===
~ threat_level += 2
"Risky play. His pistol's charge hum grows louder."
-> phase_2_resolve

=== phase_2_clues ===
He leans forward. Three new details emerge:
1. His right eye dilates unevenly—pain response.
2. The bar’s hologram flickers to a wanted poster with your face.  
3. His boot taps a rhythm matching Kowloon Station’s security code.
+ [BRIBE] -> phase_2_bribe
+ [CHARM] -> phase_2_charm
+ [THREATEN] -> phase_2_threaten
+ [HACK] -> phase_2_hack

=== phase_2_bribe ===
{
    - effective_phase2 == "BRIBE":
        ~ disposition +=2
        "Double or nothing?" He spins the cred-stick.
    - else:
        ~ disposition -=2
        "Greedy little rat." The pistol cocks.
}
-> phase_2_resolve

=== phase_2_charm ===
{
    - effective_phase2 == "CHARM":
        ~ disposition +=2
        "Still trying the sweet talk?" His damaged eye softens.
    - else:
        ~ disposition -=1
        "Save it for your funeral."
}
-> phase_2_resolve

=== phase_2_threaten ===
{
    - effective_phase2 == "THREATEN":
        ~ disposition +=2
        "Persistent." He grins, revealing filed-down metal teeth.
    - else:
        ~ disposition -=3
        ~ threat_level +=2
        "Last warning."
}
-> phase_2_resolve

=== phase_2_hack ===
{
    - effective_phase2 == "HACK":
        ~ disposition +=2
        "Again?" His neural port sparks. "You’re predictable."
    - else:
        ~ disposition -=4
        ~ threat_level +=3
        "Pathetic."
}
-> phase_2_resolve

=== phase_2_resolve ===
{
    - threat_level >= 5:
        -> death
    - disposition >= 5:
        ~ phase = 3
        -> phase_3_transition
    - else:
        ~ observed_clues = 0
        -> phase_2_clues
}

=== phase_3_transition ===
The hologram dies. Only the pistol’s charge hum fills the silence. "Last round. Make it sing."
+ [Final gambit] -> phase_3_clues

=== phase_3_clues ===  
His tells collapse into static. One clue remains:
- His remaining organic eye briefly reflects a neon dragon—your employer’s symbol.
+ [BRIBE] -> phase_3_bribe
+ [CHARM] -> phase_3_charm  
+ [THREATEN] -> phase_3_threaten
+ [HACK] -> phase_3_hack

=== phase_3_bribe ===
{
    - effective_phase3 == "BRIBE":
        ~ disposition +=1
        "Nostalgic." He pockets the stick. "The chip’s in Hong Kong Station."
    - else:
        ~ threat_level +=3
        -> death
}
-> ending

=== phase_3_charm ===
{
    - effective_phase3 == "CHARM":
        ~ disposition +=1
        "You almost remind me of..." His servo hand stills. "Locker 13."
    - else:
        ~ threat_level +=3
        -> death
}
-> ending

=== phase_3_threaten ===
{
    - effective_phase3 == "THREATEN":
        ~ disposition +=1
        "Respect." He holsters the pistol. "Kowloon Station. Don’t fail."
    - else:
        ~ threat_level +=3
        -> death
}
-> ending

=== phase_3_hack ===
{
    - effective_phase3 == "HACK":
        ~ disposition +=1
        "Enough!" He grabs his sparking skull. "Locker 13! NOW GO!"
    - else:
        ~ threat_level +=3
        -> death
}
-> ending

=== death ===
The neon dragon’s reflection in the plasma muzzle is your last sight.
-> END

=== ending ===
{
    - disposition >=6:
        CONSTRAINT BREEDS LEGENDS
    - else:
        GHOSTS DON’T GET SEQUELS
}
-> END
