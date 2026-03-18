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

native CreateGroup takes nothing returns group
native DestroyGroup takes group whichGroup returns nothing
native GroupAddUnit takes group whichGroup, unit whichUnit returns nothing
