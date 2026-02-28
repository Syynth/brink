#theme: dark
#title: DISCOVERY at CELESTIAL TOWERS
#author: Master Scoy

->02StartMenu

INCLUDE variables-global.ink
INCLUDE variables-origins.ink
INCLUDE origins-02-story.ink

=== 02StartMenu ===
# IMAGE: images/scientia-realms_1920x1080.png
    + [PRESS START]
        -> 02Prologue
    + [{02EpisodeOne}]
        -> 02Ep1
    + [{02EpisodeTwo}]
        -> 02Ep2
    + [{02EpisodeThree}]
        -> 02Ep3
    + [{02EpisodeFour}]
        -> 02Ep4
    + [{02EpisodeFive}]
        -> 02Ep5
    -> DONE
