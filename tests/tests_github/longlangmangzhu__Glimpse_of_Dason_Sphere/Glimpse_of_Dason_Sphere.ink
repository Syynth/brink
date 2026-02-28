VAR que_outs = 0//问答次数
//遥瞥戴森球 Glimpse of Dason Sphere
??? - “你好，请指定你的讲解语言。”
??? - "Hello, please specify your explanation language."
+[中文]游客 - 你略过那些长得惊人的条款说明，勾选了几个项目以后，智能导游开始流畅地吐出你熟悉的语言。
->ch
+[English]Tourists - you skip the stunningly long terms and conditions, tick a few items, and the smart guide starts spitting out the language you know.
->en
===ch
导游 - “大家好，我是你们的导游X823-5796Ⅱ。欢迎来到原月球基地。接下来由我来带领大家详细了解位于太阳系的戴森球结构SolarD-Ⅰ。”
+[你想先了解一下什么是戴森球。]游客 - “什么是戴森球？”
--~que_outs++
->ch1
*[你更好奇戴森球长什么样。]游客 - “{~我不想听|这我知道}，我就想看看戴森球到底长啥样？”
->ch30
*[你只想知道你能不能上去看看。]游客 - “{~我只想知道我能不能亲自上去看看。|那我能不能亲自上去看看？}”
->ch41
*[你不需要导游讲解。]
->ch42
=ch1
-导游 - “戴森根据马尔萨斯的相关理论，从宇宙中维持长期存在的文明其能源需求必然不断上升的逻辑出发，提出当文明发展到一定等级时，必然会开发利用恒星能量的。他构想出以行星为材料可以建筑一个人工生物圈，并利用恒星能源来实现自给。……”
+[继续]
-导游 - “随后由卡尔达肖夫，引述戴森球一例可以作为其划分宇宙文明的三种文明中由Ⅰ型文明向Ⅱ型文明跃升的方法之一，也就是说，Ⅱ型文明应当可以利用该类巨构实现对恒星能量的接受与使用。通过卡尔达肖夫等级，戴森球工程被纳入到星系文明学范畴，并通过后来的一些科幻作品进一步地为人们所熟知。……”
+[继续]
-导游 - “戴森本人则认为，一个包围太阳的坚固的壳或环从力学上是不可行的。他曾设想的一个形式是由松散的太阳能收集器或独立环绕太阳轨道的卫星云组成。这种“软”的戴森球又被称为戴森云。……”
+[继续]
-导游 - “虽然后来仍有科幻小说家们坚信构筑一个坚实的戴森壳是有可能的，但显然，化整为零，采取较小单位的太阳能接收器构成一个或多个庞大的阵群结构是更容易的。因为我们并不需要真正的无序结构，我们需要内禀一种秩序用以日常维护……”
+[你想了解更多。]游客 - “那……”->ch2
+[你等不及想看戴森球了。]游客 - “好了好了，{~我听够够的了，|}让我亲眼看看就好。”
->ch30
=ch2
-(opts)
*[建造戴森球对原太阳系产生了什么影响？]游客 - “建造戴森球对原太阳系产生了什么影响？”
导游 - 导游示意辅助系统投影太阳系。“这是旧的太阳系空间地图。当初戴森球的建设材料主要来源自内太阳系，包括水星，金星，地球及月球，火星这几颗质量比较大的星球。建成后，戴森球最外层已经超出原有水星轨道，而原来的内太阳系各星球因为不同的质量损耗被标记并在新的星图上作包括除名、降级等相应的处理。”
--~que_outs++
*[戴森球是如何输送能量的？]游客 - “戴森球是如何输送能量的？”
导游 - “目前这颗戴森球仍是以激光束形式向外待供能设施供能。”
--~que_outs++
*[戴森球是怎么建造起来的？]游客 - “戴森球是怎么建造起来的？”
导游 - “需要说明的是，戴森球项目立项之前，我们的祖先——人类——就已经技术奠基了数千年。但所需要的物质单从质量上来说就是海量的。要满足建设需要，将行星“整体拆解”。我们的祖先首先利用了水星——这颗距离太阳最近且仅存有一个较为稀薄的外溢层——耗费十余年在其表面开发，试验并成功发射多批火箭将太阳帆送上近日轨道，粉碎了“戴森球是不可能”的谣言。而后项目有条不紊的开展，经过百余年，终于建成如今十层戴森球。”
--~que_outs++
*[建设戴森球的关键技术包括哪些？]游客 - “建设戴森球的关键技术包括哪些？”
导游 - “戴森球的建设需要利用材料学、能源学、物理学、数学、信息学、工程学等诸多学科的知识。总的来说，数学和物理学等学科可以帮助相关理论顺利产出和得到验证，信息学和工程学等学科帮助理论转化和管理运行。”
--~que_outs++
--->ch29
+{loop} [你不继续问了。]->ch5
-(loop)
 {->opts|->opts|->opts|}
*[你问够了。]
->ch5
=ch5
导游 - 导游看没有人继续提问，停顿了一下。进入下一项。
->ch30
=ch6
->->
=ch28
~que_outs++
导游 - “在建设这一工程首先要保证一系列稳定性：力学上，要满足引力、材料应力，同时承受并抵抗恒星和自身作用导致结构失效；热力学上，面对光照差异导致的巨大温度梯度，需要在极端高低温状态下保持足够的材料结构强度并在合适的工作温度范围正常工作，且在温度变化下弱化材料的形变与裂纹现象；在辐射与粒子方面，恒星发出的恒星风、高能粒子流和强烈辐射会对结构造成侵蚀和破坏，需要使用能够抵挡这些效应的材料和技术。”
*[继续]
导游 - “另外就戴森球的动态建设运行上，戴森球在面对太空中高速飞行的微小陨石和尘埃对结构的持续损伤，必须具备一定的防御与自修复能力；需要精确设计并控制戴森球各层级在轨运行，防止各层级之间的相互碰撞或因轨道共振导致的不稳定现象；结构自转产生的离心力和潮汐力也需要得到平衡，以免引发内部应力和外部扰动。在建造过程中，需要逐步构建并保持临时结构的稳定性。所幸，在建设SolarD-Ⅰ的过程中，没有大的工程事故的记录。”
->ch29
=ch29
*{not ch28 and not ch6}[继续]->ch28
*[材料学？]游客 - “那材料学？”
导游 - “在材料学方面，首先需要填满巨量的材料缺口，也即要求高效的矿石开采与精炼技术来获取原料。满足了数量，还要考虑质量——需要开发高强度、高稳定性、低损耗率的材料，针对性地开发并量产与功能相适宜的材料，特别是在太阳能转换与传输系统方面所需要的高效能材料。”
--~que_outs++
--->ch6->ch29
*[能源学？]游客 - “那能源学？”
导游 - “在能源学方面，需要开发能源收集转化、储存、传输技术，在能源管理系统的调配下尽可能降低能源损耗。就目前而言，在太阳系的外层空间，比如说在奥尔特云的一些基地，为了更好的远距离传输能源，在线路中途会设立一至二个能源中继站，负责接收、临时储存和重新发射能量，以确保能源传输的连续性。”
--~que_outs++
--->ch6->ch29
*[物流运输……]游客 - “那如何物流运输？”
导游 - “在物流运输方面，要求建设并调配地对地、地对空、空对空运输体系，并设计和制造高负载、高效低耗、自适应的新型运输工具。这个联合物流运输体系，不仅在整个戴森球建设周期内都能正常运作，并进一步帮助我们向太阳系外扩张。”
--~que_outs++
--->ch6->ch29
*[管理？]游客 - “那如何管理？”
导游 - “在管理方面，简单可以分为三期。一期先行建设，二期建设与运行并存，三期主要是长期的运维。戴森球项目施工时间长，施工环境差异大，就所处工作环境的不同要求相关运行、维护、自动化技术适配。在戴森球的核心区有一台光脑负责统筹调配相关事项，并及时与上位主脑沟通联系。”
--~que_outs++
--->ch6->ch29
*[这样啊……]游客 - “这样啊……那……”
->ch2
=ch30
导游 - 导游示意辅助系统升起太空舱窗帘。“大家顺着我手指的地方看，我们现在可以看到的球状结构就是戴森球。正是由它层层包裹，经过缜密计算，它从内到外可分为十层，结构类似于一种中国的工艺品象牙套球。多说一句，大家在博物馆可以找到这种工艺品，如果你们没见过的话可以上光网上搜索一下，真的很漂亮。”
+[你出神地望着戴森球，仿佛忘却时间……]
-导游 - “戴森球各层拥有各自的旋转逻辑，能够在虽然有遮挡的情况，仍较单层蛋壳结构与多层环带结构的能源利用率高出不少，实现太阳能捕捉的最大化。戴森球项目的建成，标志着人类真正把太阳这颗对我们人类来说意义重大的恒星，作为天然的核聚变反应堆来充分利用。”
+[……]
->ch43
=ch41
导游 - “出舱需要出示证照资格。”
*[……]{~游客 - “好吧。”|游客 - “哦。”|}
{que_outs>9:->->}
->ch30
=ch42
*[出示证照，你可以自行外出看看。]游客 - 你出示了相关证照，可以短暂地舱外行走一会儿。你领取宇航服并穿戴整齐，开启舱门，一步迈出。你面前是太空中无数的星斑。你看向太阳的方向，它被深藏在巨大的层壳结构中。太阳似乎隐形了，又或者它根本不在那里。还是压根就没有太阳存在？你的眼底似乎感受不到那个方向传来的可见光频段的讯息。就像背景一样漆黑，夜的幽邃，美得恰如其分！这一刻，你被人力所震撼。人竟能做到如此境地！你挪不开眼睛，忘却了时间……你的思绪在飘远……
# IMAGE: DSwithinspace.jpg
{que_outs>9:->->}
//{que_outs<=9}
# CLASS: end
达成结局1
->END//BE1
=ch43
导游 - “差不多是饭点了。各位可以移步3号餐厅或5号餐厅用餐。今天1号餐厅有提供特色面点月饼哦！当然也可以选择留下来继续欣赏太空。希望我的服务能给各位带来愉快……”
*{que_outs>9}[你等到人群散的差不多了，拦住导游。]游客 - “我能不能出舱看看？”
->ch99
*{que_outs<=9}->ch50
=ch50
# CLASS: end
达成结局2
->END//GE1
=ch99
->ch41->
导游 - “但是程序上我应该可以提交一个申请，安排在专业人员的看护下临时出舱……”
*[可以吗？]游客 - “可以吗？”
-导游 - “可以的。只是自我工作以来没遇到过……”
*[拜托了！这真的对我……]游客 - “拜托了！这真的对我……”
-导游 - “不一样？”
*[舱内舱外不一样！]游客 - “不一样！”
-导游 - “好吧。下午你会看到申请的结果。”
->ch100
=ch100
*[时间流逝中……]
下午。
导游 - 导游把临时证照交给你。
-->ch42->
*[时间流逝中……]
-不知过了多久。
??? - “回回神。”一个不算熟悉的女声，很英气。
*[你下意识的转头看向你的看护者。]游客 - 你下意识的转头看向你的看护者。
-看护者 - “那确实是人类的奇迹。不过你们也是。收拾你的好奇心，前往下个地方吧。”
*[你离开。没有回头。]游客 - 你背对着星空，跟着看护者，向来时的舱门走去。没有回头。
#CLASS: end
达成结局3
->END//GE2
===en
-Guide - "Hi, I'm your guide, X823-5796II, and welcome to the original Moon base, where I'll be taking you through the Dyson sphere structure in the Solar System, SolarD-I."
+[You want to know what Dyson sphere is first.]Tourists - "What is Dyson Sphere?"
--~que_outs++
->en1
*[You're more curious about what Dyson sphere looks like.]Tourists - "{~I don't want to hear it,|I know that}，I just want to see what Dyson sphere looks like?"
->en30
*[You just want to know if you can go there.]Tourists - "{~I just want to know if I can go there and see for myself.|Can I go there and see for myself?}”
->en41
*[You Don't need a guide to explain.]
->en42
=en1
-Guide - "According to Malthus's theory, Dason, based on the logic that the energy needs of long-standing civilizations in the universe must rise, argues that when a civilization reaches a certain level, it must exploit the energy of the stars. He conceived of building an artificial biosphere out of planetary materials and using the energy of stars to achieve self-sufficiency..."
+[Continue]
-Guide - "The Nikolai Kardashev then quotes Dyson sphere as one of the ways in which he could classify the three civilizations of the universe from Type I to type II, that is to say, type II civilizations should be able to use this type of macrostructure to receive and use the energy of stars. Through Nikolai Kardashev, the Dyson Sphere Project was incorporated into the field of galactic civilisation and further popularized by subsequent sci-fi works..."
+[Continue]
--Guide - "Dyson himself argued that a solid shell or ring around the sun was not mechanically feasible. One form he envisioned would consist of loose solar collectors or satellite clouds orbiting independently of the Sun. This "Soft" Dyson sphere is also known as the Dyson Cloud..."
+[Continue]
-Guide - "Although some science fiction writers would later insist that it was possible to build a solid Dyson shell, it was clear that, in pieces, it is easier to take smaller units of solar receivers to form one or more large arrays. Because we don't need a real disorder structure, we need an inherent order for daily maintenance..."
+[You want to know more.]Tourist - "Emm..."->en2
+[You Can't wait to see Dyson sphere.]Tourists - "Okay, okay, {~I've heard enough, |}just let me see it with my own eyes."
->en30
=en2
-(opts)
*[What effect did the construction of Dyson sphere have on the proto-solar system?]Tourist - "What effect did the construction of Dyson sphere have on the proto-solar system?"
Guide - Guide indicating the auxiliary system to project the Solar System. "This is an old space map of the Solar System. Dyson sphere's building materials are derived primarily from the inner solar system, including Mercury, Venus, Earth, the Moon, and Mars, the more massive planets. When completed, Dyson sphere's outermost layer was already beyond the orbit of Mercury, and the original inner Solar System stars were marked for different mass losses and processed in the new map, including de-listing and demotion."
--~que_outs++
*[How does Dyson Sphere Channel its energy?]Tourist - "How does Dyson Sphere Channel its energy?"
Guide - "For now, the Dyson sphere is still powered by a laser beam."
--~que_outs++
*[How did Dyson Sphere Build It?]Tourist - "How did Dyson Sphere Build It?"
Guide - "It's important to note that, humans, were laying the foundations of technology for thousands of years before project Dyson sphere was even built. But the amount of material needed is massive in terms of mass alone. To meet construction needs, the planet will be“Taken apart as a whole.”. Our ancestors first took advantage of mercury, the closest planet to the sun with only a thinner outer layer, and spent more than a decade developing it on the surface, the test and successful launch of several rockets to put the solar sail into near-earth orbit shattered rumors that Dyson sphere was impossible. Then the project was carried out in an orderly manner. After more than a hundred years, the 10-storey Dyson sphere was finally completed."
--~que_outs++
*[What are the key technologies that built Dyson sphere?]Tourist - "What are the key technologies that built Dyson sphere?"
Guide - "Dyson sphere's construction required knowledge of materials science, Energy Science, physics, Mathematics, information science, engineering and many other disciplines. In general, disciplines such as mathematics and physics help to produce and validate relevant theories, while disciplines such as Informatics and engineering help to transform and manage theories.”
--~que_outs++
--->en29
+{loop} [You stop asking.]->en5
-(loop)
 {->opts|->opts|->opts|}
*[You've asked enough.]
->en30
->en5
=en5
Guide - The guide paused when he saw that no one was asking any more questions. Move on to the next item.
->en30
=en6
->->
=en28
~que_outs++
Guide - "In order to build this project, a series of stability must be ensured: in mechanics, gravity and material stress must be satisfied, and the structure must withstand and resist the action of the star and itself, leading to the failure of the structure; in thermodynamics, in the face of the huge temperature gradient caused by light difference, it is necessary to maintain sufficient structural strength of the material at extreme high and low temperatures and work normally within the appropriate temperature range, and to weaken the deformation and crack phenomenon of the material under the change of temperature; in terms of radiation and particles, the stellar wind, high-energy particle flow and intense radiation emitted by the star will cause erosion and damage to the structure, and the use of materials and technologies to resist these effects is required."
*[Continue]
Guide - "In addition to Dyson sphere's dynamic construction and operation, Dyson sphere must be able to defend and repair itself against the constant damage caused by small meteorites and dust flying at high speed in space, precisely design and control the operation of the Dyson ball layers in orbit to prevent collisions between the layers or instability caused by orbital resonance, and balance the centrifugal and tidal forces generated by the structure's rotation to avoid internal stresses and external disturbances. During the construction process, the temporary structure needs to be built step by step and the stability of the temporary structure needs to be maintained. Fortunately, during the construction of SolarD-I, no major engineering accidents were recorded."
->en29
=en29
*{not en28 and not en6}[Continue]->en28
*[Materials science?]Tourist - "What about materials science?"
Guide - "In materials science, there is a huge material gap that needs to be filled first, which requires efficient ore mining and refining techniques to obtain raw materials. Meet quantity and consider quality-develop materials with high strength, high stability and low loss rate, and develop and mass produce materials that are appropriate for function, especially for high efficiency materials needed for solar energy conversion and transmission systems."
--~que_outs++
--->en6->en29
*[energy?]Tourist - "What about energy?"
Guide - "In energy, there is a need to develop technologies for energy collection, conversion, storage, and transmission to minimize energy consumption through the deployment of energy management systems. For the time being, in the outer space of the Solar System, for example at bases in Oort cloud, one or two relay stations will be set up along the way to receive, temporarily store and re-transmit energy for better long-distance transmission to ensure continuity of energy transmission.”
--~que_outs++
--->en6->en29
*[logistics transport...]Tourist - "What about logistics?"
Guide - "In logistics, it requires the construction and deployment of ground-to-ground, ground-to-air, air-to-air transport systems, and the design and manufacture of high-load, efficient, low-cost, adaptive new transport vehicles This combined logistics system will not only function throughout the entire Dyson sphere construction cycle, but will also further help us expand beyond our solar system."
--~que_outs++
--->en6->en29
*[management?]Tourist - "How is it managed?"
Guide - "In terms of management, simplicity can be divided into three phases. The first phase construction, the second phase construction and Operation Coexist, the third phase is mainly long-term operation and maintenance. Dyson sphere project construction time is long, construction environment is different, on the different requirements of the work environment related to operation, maintenance, automation adaptation. In Dyson Sphere's core area, there is a optical computer that co-ordinates matters and communicates with the top brain in a timely manner."
--~que_outs++
--->en6->en29
*[Well...]Tourist - "Oh...Yea..."
->en2
=en30
Guide - Guide indicating the auxiliary system to raise the hatch curtains. “If you follow where I'm pointing, the spherical structure that we see here is Dyson sphere. It is wrapped layer by layer, after careful calculation, it can be divided into ten layers from the inside to the outside, the structure is similar to a Chinese handicraft ivory ball. A word more, you can find this kind of handicraft in the museum, if you haven't seen it, you can go online and search it, it's really beautiful."
+[You look at Dyson sphere as if you've lost track of time...]
-Guide - "Dyson sphere's layers have their own rotational logic that allows them to maximize their solar capture over a single-layer eggshell structure and a multi-layer ring structure, even if the efficient energy use is obscured. The completion of Project Dyson sphere is a sign that humanity is truly taking full advantage of the Sun as a natural nuclear fusion power.”
+[...]
->en43
=en41
Guide - "Certificate qualification is required for extravehicular travel."
*[...]{~Tourist - "Okay."|Tourist - "O..."|}
->en30
=en42
*[Show your license. And you can go out and see for yourself.]Tourist - You have shown your relevant license for a short extravehicular walk. You get your spacesuit, you get dressed, you open the door, you step out. In front of you are countless stars in space. You look in the direction of the Sun, and it's buried deep in this huge shell structure. The Sun seems to be invisible, or it's not there at all. Or is there no sun at all? The bottom of your eye doesn't seem to be able to pick up the information coming from that direction in the visible light band. As dark as the background, the dark night, the United States is just right! This moment, you are shocked by human power. How can a man do such a thing! You Can't keep your eyes off it... But time is passing, and Your Mind is drifting away...
# IMAGE: DSwithinspace.jpg
{que_outs>9:->->}
//{que_outs<=9}
#CLASS: end
Finale 1
->END//BE1
->END
=en43
Guide - "It's almost time for eating. You can move to restaurant No. 3 or restaurant No. 5. Restaurant No. 1 is serving special mooncakes with pastries today! Or you can stay and enjoy space. I hope my service will bring you pleasure..."
*{que_outs>9}[You wait until the crowd has dispersed, stop the guide.]Tourist - "Can I go out and have a look?"
->en99
*{que_outs<=9}->en50
=en50
#Class: end
Finale 2
->END//GE1
=en99
->en41->
Guide-“But procedurally I should be able to submit a request to arrange for a temporary egress under professional supervision...”
*[May I?]Tourist - "May I?"
-Guide - "Yes. It's just that I haven't Met..."
*[Please! For me, this is really...]Tourist - "Please! For me, this is really..."
-Guide - "Not the same?"
*[Inside and outside the cabin is different!]Tourist - "Not the same!"
-Guide - "All Right. You'll get the results of your application this afternoon."
->ch100
=ch100
*[Time passes...]
Afternoon.
Guide - The guide gives you the temporary license.
-->en42->
*[Time passes...]
-Don't know how long it's been.
??? - "Come back." An unfamiliar female voice, very heroic.
*[You subconsciously turn to look at your caregiver.]Tourist - You subconsciously turn to look at your caregiver.
-Caregiver - "It is a miracle of mankind. But so are you. Gather your curiosity and go to the next place."
*[You leave. No turning back.]Tourist - You turn your back on the starry sky, follow the caregiver, and walk towards the incoming hatch. No turning back.
#Class: end
Finale 3
->END//GE2
//感谢戴森球计划
//Thanks to Dason Sphere Program
//以上GoDS-0.05v
//That is all.Bye~