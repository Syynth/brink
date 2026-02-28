#theme: dark
#title: WANDERING in the FOREST
#author: Master Scoy

->01StartMenu

INCLUDE variables-global.ink
INCLUDE variables-origins.ink
INCLUDE origins-01-story.ink

=== 01StartMenu ===
    + [PRESS START]
        -> 01Prologue
    + [{01EpisodeOne}]
        -> 01Ep1
    + [{01EpisodeTwo}]
        -> 01Ep2
    + [{01EpisodeThree}]
        -> 01Ep3
    + [{01EpisodeFour}]
        -> 01Ep4
    + [{01EpisodeFive}]
        -> 01Ep5
    -> DONE
