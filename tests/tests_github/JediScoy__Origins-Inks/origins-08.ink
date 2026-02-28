#theme: dark
#title: HEROES of ARRAKIS
#author: Master Scoy

->08StartMenu

INCLUDE variables-global.ink
INCLUDE variables-origins.ink
INCLUDE origins-08-story.ink


=== 08StartMenu ===
    + [PRESS START]
        ->08Prologue
    + [{08EpisodeOne}]
        ->08Ep1
    + [{08EpisodeTwo}]
        ->08Ep2
    + [{08EpisodeThree}]
        ->08Ep3
    + [{08EpisodeFour}]
        ->08Ep4
    + [{08EpisodeFive}]
      ->08Ep5
-> DONE
