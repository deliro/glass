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

// bj_DEGTORAD = 0.01745329 — use 3.14159/180.0 in Glass code instead
