// Minimal common.j stub for pjass validation of Glass output
type agent extends handle
type widget extends agent
type unit extends widget
type item extends widget
type destructable extends widget
type player extends agent
type timer extends agent
type trigger extends agent
type triggercondition extends agent
type triggeraction extends agent
type event extends agent
type force extends agent
type group extends agent
type location extends handle
type rect extends agent
type boolexpr extends agent
type sound extends agent
type effect extends agent
type quest extends agent
type unitpool extends handle
type itempool extends handle
type dialog extends agent
type button extends agent
type texttag extends handle
type lightning extends handle
type image extends handle
type ubersplat extends handle
type hashtable extends agent
type region extends agent
type gamecache extends agent
type fogmodifier extends agent
type multiboard extends agent
type multiboarditem extends handle
type trackable extends agent
type timerdialog extends agent
type leaderboard extends agent

native CreateTrigger takes nothing returns trigger
native DestroyTrigger takes trigger t returns nothing
native TriggerRegisterTimerEvent takes trigger t, real timeout, boolean periodic returns event
native TriggerAddAction takes trigger t, code actionFunc returns triggeraction
native TriggerAddCondition takes trigger t, boolexpr condition returns triggercondition

native CreateTimer takes nothing returns timer
native DestroyTimer takes timer t returns nothing
native TimerStart takes timer t, real timeout, boolean periodic, code handlerFunc returns nothing
native GetExpiredTimer takes nothing returns timer

native InitHashtable takes nothing returns hashtable
native SaveInteger takes hashtable table, integer parentKey, integer childKey, integer value returns nothing
native LoadInteger takes hashtable table, integer parentKey, integer childKey returns integer
native FlushChildHashtable takes hashtable table, integer parentKey returns nothing
native GetHandleId takes handle h returns integer

native Player takes integer number returns player
native GetLocalPlayer takes nothing returns player
native DisplayTimedTextToPlayer takes player toPlayer, real x, real y, real duration, string message returns nothing

native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native RemoveUnit takes unit whichUnit returns nothing
native GetUnitX takes unit whichUnit returns real
native GetUnitY takes unit whichUnit returns real
native SetUnitX takes unit whichUnit, real newX returns nothing
native SetUnitY takes unit whichUnit, real newY returns nothing
native SetUnitPosition takes unit whichUnit, real newX, real newY returns nothing
native GetUnitFacing takes unit whichUnit returns real
native UnitDamageTarget takes unit whichUnit, widget target, real amount, boolean attack, boolean ranged, integer attackType, integer damageType, integer weaponType returns boolean
native AddSpecialEffect takes string modelName, real x, real y returns effect
native AddSpecialEffectTarget takes string modelName, widget targetWidget, string attachPointName returns effect
native DestroyEffect takes effect whichEffect returns nothing

native CreateGroup takes nothing returns group
native DestroyGroup takes group whichGroup returns nothing
native GroupAddUnit takes group whichGroup, unit whichUnit returns nothing

native SaveReal takes hashtable table, integer parentKey, integer childKey, real value returns nothing
native SaveStr takes hashtable table, integer parentKey, integer childKey, string value returns nothing
native SaveBoolean takes hashtable table, integer parentKey, integer childKey, boolean value returns nothing
native LoadReal takes hashtable table, integer parentKey, integer childKey returns real
native LoadStr takes hashtable table, integer parentKey, integer childKey returns string
native LoadBoolean takes hashtable table, integer parentKey, integer childKey returns boolean
native HaveSavedInteger takes hashtable table, integer parentKey, integer childKey returns boolean
native HaveSavedReal takes hashtable table, integer parentKey, integer childKey returns boolean
native HaveSavedString takes hashtable table, integer parentKey, integer childKey returns boolean
native HaveSavedBoolean takes hashtable table, integer parentKey, integer childKey returns boolean
native RemoveSavedInteger takes hashtable table, integer parentKey, integer childKey returns nothing
native RemoveSavedReal takes hashtable table, integer parentKey, integer childKey returns nothing
native RemoveSavedString takes hashtable table, integer parentKey, integer childKey returns nothing
native RemoveSavedBoolean takes hashtable table, integer parentKey, integer childKey returns nothing
native SaveUnitHandle takes hashtable table, integer parentKey, integer childKey, unit whichUnit returns boolean
native LoadUnitHandle takes hashtable table, integer parentKey, integer childKey returns unit
native GetRandomInt takes integer lowBound, integer highBound returns integer
native GetRandomReal takes real lowBound, real highBound returns real

native I2S takes integer i returns string
native S2I takes string s returns integer
native I2R takes integer i returns real
native R2I takes real r returns integer
native R2S takes real r returns string
native S2R takes string s returns real
native StringLength takes string s returns integer
native SubString takes string source, integer start, integer end returns string
native StringCase takes string source, boolean upper returns string
native StringHash takes string s returns integer
native SquareRoot takes real r returns real
native Sin takes real radians returns real
native Cos takes real radians returns real
native Atan2 takes real y, real x returns real

// --- Player ---
native GetPlayerId takes player whichPlayer returns integer
native GetTriggerPlayer takes nothing returns player
native GetOwningPlayer takes unit whichUnit returns player
native GetPlayerName takes player whichPlayer returns string
native SetPlayerName takes player whichPlayer, string name returns nothing
native GetPlayerState takes player whichPlayer, integer whichPlayerState returns integer
native SetPlayerState takes player whichPlayer, integer whichPlayerState, integer value returns nothing

// --- Unit (extended) ---
native SetUnitFacing takes unit whichUnit, real facingAngle returns nothing
native GetUnitTypeId takes unit whichUnit returns integer
native GetUnitState takes unit whichUnit, integer whichUnitState returns real
native SetUnitState takes unit whichUnit, integer whichUnitState, real newVal returns nothing
native KillUnit takes unit whichUnit returns nothing
native SetUnitOwner takes unit whichUnit, player whichPlayer, boolean changeColor returns nothing
native SetUnitAnimation takes unit whichUnit, string whichAnimation returns nothing
native PauseUnit takes unit whichUnit, boolean flag returns nothing
native ShowUnit takes unit whichUnit, boolean show returns nothing
native SetUnitInvulnerable takes unit whichUnit, boolean flag returns nothing
native IsUnitType takes unit whichUnit, integer whichUnitType returns boolean
native SetUnitMoveSpeed takes unit whichUnit, real speed returns nothing
native IssueImmediateOrder takes unit whichUnit, string order returns boolean
native IssuePointOrder takes unit whichUnit, string order, real x, real y returns boolean
native IssueTargetOrder takes unit whichUnit, string order, widget targetWidget returns boolean
native PingMinimap takes real x, real y, real duration returns nothing

// --- Hero ---
native GetHeroLevel takes unit whichHero returns integer
native SetHeroLevel takes unit whichHero, integer level, boolean showEyeCandy returns nothing
native GetHeroXP takes unit whichHero returns integer
native AddHeroXP takes unit whichHero, integer xpToAdd, boolean showEyeCandy returns nothing
native GetHeroStr takes unit whichHero, boolean includeBonuses returns integer
native GetHeroAgi takes unit whichHero, boolean includeBonuses returns integer
native GetHeroInt takes unit whichHero, boolean includeBonuses returns integer
native SetHeroStr takes unit whichHero, integer newStr, boolean permanent returns nothing
native SetHeroAgi takes unit whichHero, integer newAgi, boolean permanent returns nothing
native SetHeroInt takes unit whichHero, integer newInt, boolean permanent returns nothing
native ReviveHero takes unit whichHero, real x, real y, boolean doEyecandy returns boolean

// --- Ability ---
native UnitAddAbility takes unit whichUnit, integer abilityId returns boolean
native UnitRemoveAbility takes unit whichUnit, integer abilityId returns boolean
native GetUnitAbilityLevel takes unit whichUnit, integer abilcodeId returns integer
native SetUnitAbilityLevel takes unit whichUnit, integer abilcodeId, integer level returns integer
native IncUnitAbilityLevel takes unit whichUnit, integer abilcodeId returns integer
native UnitMakeAbilityPermanent takes unit whichUnit, boolean permanent, integer abilityId returns boolean

// --- Item ---
native CreateItem takes integer itemid, real x, real y returns item
native RemoveItem takes item whichItem returns nothing
native UnitAddItem takes unit whichUnit, item whichItem returns boolean
native UnitAddItemById takes unit whichUnit, integer itemId returns item
native GetItemName takes item whichItem returns string
native GetItemCharges takes item whichItem returns integer
native SetItemCharges takes item whichItem, integer charges returns nothing
native GetItemLevel takes item whichItem returns integer
native SetItemDroppable takes item whichItem, boolean flag returns nothing

// --- Timer (extended) ---
native PauseTimer takes timer whichTimer returns nothing
native ResumeTimer takes timer whichTimer returns nothing
native TimerGetRemaining takes timer whichTimer returns real
native TimerGetElapsed takes timer whichTimer returns real
native TimerGetTimeout takes timer whichTimer returns real

// --- Group (extended) ---
native GroupRemoveUnit takes group whichGroup, unit whichUnit returns nothing
native GroupClear takes group whichGroup returns nothing
native IsUnitInGroup takes unit whichUnit, group whichGroup returns boolean
native FirstOfGroup takes group whichGroup returns unit
native GroupEnumUnitsInRange takes group whichGroup, real x, real y, real radius, boolexpr filter returns nothing

// --- UI ---
native ClearTextMessages takes nothing returns nothing
native CreateTextTag takes nothing returns texttag
native DestroyTextTag takes texttag t returns nothing
native SetTextTagText takes texttag t, string s, real height returns nothing
native SetTextTagPos takes texttag t, real x, real y, real heightOffset returns nothing
native SetTextTagColor takes texttag t, integer red, integer green, integer blue, integer alpha returns nothing
native SetTextTagVelocity takes texttag t, real xvel, real yvel returns nothing
native SetTextTagLifespan takes texttag t, real lifespan returns nothing
native SetTextTagPermanent takes texttag t, boolean flag returns nothing
native SetTextTagFadepoint takes texttag t, real fadepoint returns nothing

// --- Camera ---
native SetCameraPosition takes real x, real y returns nothing
native PanCameraTo takes real x, real y returns nothing
native PanCameraToTimed takes real x, real y, real duration returns nothing
native SetCameraField takes integer whichField, real value, real duration returns nothing
native ResetToGameCamera takes real duration returns nothing
native GetCameraTargetPositionX takes nothing returns real
native GetCameraTargetPositionY takes nothing returns real

// --- Sound ---
native CreateSound takes string fileName, boolean looping, boolean is3D, boolean stopwhenoutofrange, integer fadeInRate, integer fadeOutRate, string eaxSetting returns sound
native StartSound takes sound soundHandle returns nothing
native StopSound takes sound soundHandle, boolean killWhenDone, boolean fadeOut returns nothing
native KillSoundWhenDone takes sound soundHandle returns nothing
native SetSoundVolume takes sound soundHandle, integer volume returns nothing
native SetSoundPosition takes sound soundHandle, real x, real y, real z returns nothing

// --- Rect ---
native Rect takes real minx, real miny, real maxx, real maxy returns rect
native RemoveRect takes rect whichRect returns nothing
native GetRectCenterX takes rect whichRect returns real
native GetRectCenterY takes rect whichRect returns real
native GetRectMinX takes rect whichRect returns real
native GetRectMinY takes rect whichRect returns real
native GetRectMaxX takes rect whichRect returns real
native GetRectMaxY takes rect whichRect returns real

// --- Destructable ---
native CreateDestructable takes integer objectid, real x, real y, real face, real scale, integer variation returns destructable
native RemoveDestructable takes destructable d returns nothing
native KillDestructable takes destructable d returns nothing
native GetDestructableLife takes destructable d returns real
native SetDestructableLife takes destructable d, real life returns nothing
native GetDestructableMaxLife takes destructable d returns real
native SetDestructableMaxLife takes destructable d, real max returns nothing

// --- Multiboard ---
native CreateMultiboard takes nothing returns multiboard
native DestroyMultiboard takes multiboard lb returns nothing
native MultiboardDisplay takes multiboard lb, boolean show returns nothing
native MultiboardSetTitleText takes multiboard lb, string label returns nothing
native MultiboardSetColumnCount takes multiboard lb, integer count returns nothing
native MultiboardSetRowCount takes multiboard lb, integer count returns nothing
native MultiboardGetItem takes multiboard lb, integer row, integer column returns multiboarditem
native MultiboardSetItemValue takes multiboarditem mbi, string val returns nothing
native MultiboardSetItemWidth takes multiboarditem mbi, real width returns nothing
native MultiboardReleaseItem takes multiboarditem mbi returns nothing

// Event types for unit events
type playerunitevent extends handle
type unitevent extends handle

// Event response natives
native GetTriggerUnit takes nothing returns unit
native GetAttacker takes nothing returns unit
native GetKillingUnit takes nothing returns unit
native GetSpellAbilityId takes nothing returns integer
native GetSpellTargetUnit takes nothing returns unit
native GetManipulatedItem takes nothing returns item
native GetItemTypeId takes item whichItem returns integer
native TriggerRegisterPlayerUnitEvent takes trigger whichTrigger, player whichPlayer, playerunitevent whichPlayerUnitEvent, boolexpr filter returns event
native TriggerRegisterUnitEvent takes trigger whichTrigger, unit whichUnit, unitevent whichEvent returns event

// bj_DEGTORAD = 0.01745329 — use 3.14159/180.0 in Glass code instead

globals
    constant integer UNIT_STATE_LIFE = 0
    constant integer UNIT_STATE_MAX_LIFE = 2
    constant integer UNIT_STATE_MANA = 3
    constant integer UNIT_STATE_MAX_MANA = 4
    constant integer bj_MAX_PLAYER_SLOTS = 24
    constant integer PLAYER_STATE_RESOURCE_GOLD = 1
    constant integer PLAYER_STATE_RESOURCE_LUMBER = 2
endglobals
