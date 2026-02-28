	->MainMenu
	=== MainMenu
+ Mon Histoire ->MonHistoireStory
+ Super Histoire ->SuperHistoireStory
=== MonHistoireStory
VAR ONE_indice = false

Vous voyez un bel indice.

* {TWO_variable} J'ai déjà joué à l'autre jeu
# REINIT:
-> MainMenu
* Ramasser l'indice
~ONE_indice=true
* Laisser l'indice

-
Qui est le coupable ?

 + C'est moi !
 Ah, zut je me suis trompé !
 PERDU
 
 + C'est vous !
{ ONE_indice == true :
        Et je peux le prouver avec cet indice !
        GAGNE
    - else:
        Mais je ne peux pas le prouver, hélas !
        PERDU
}
-
Ainsi s'achève les aventures de l'indice à ramasser.

    # REINIT:
-> MainMenu

=== SuperHistoireStory
VAR TWO_variable = false

Mais attendez, je ne l'ai pas déjà faite cette histoire ?
    -> TWO_indice
=== TWO_indice
~ TWO_variable = true
Il y avait une histoire d'indice, mais là je ne le trouve plus.
# REINIT:
-> MainMenu
